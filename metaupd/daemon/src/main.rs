//! metaupd's D-Bus service.
//!
//! Exposes the engine to desktops and adds no update logic. Callers cannot choose
//! the release source, the trust directory or any path.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use clap::Parser;
use meta_common::{Platform, SystemRunner};
use metaupd::engine::{EngineError, host_from_platform};
use metaupd::state::State;
use metaupd::store::Paths;
use metaupd::{Engine, METAUPD_VERSION, apply, history};
use metaupd_github::{CommandFetcher, DirectorySource, GithubSource, ReleaseSource};
use metaupd_metadata::{Channel, Timestamp};
use zbus::fdo::RequestNameFlags;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, interface};

const NAME: &str = "io.github.tnuproject.MetaUpd1";
const PATH: &str = "/io/github/tnuproject/MetaUpd1";

#[derive(Parser)]
#[command(name = "metaupdd", version = METAUPD_VERSION, about = "META update service")]
struct Cli {
    /// Directory for state, history and downloads
    #[arg(long, value_name = "DIR")]
    state_dir: Option<PathBuf>,
    /// Directory holding the trusted root metadata
    #[arg(long, value_name = "DIR")]
    trust_dir: Option<PathBuf>,
    /// Use a local release directory instead of GitHub Releases
    #[arg(long, value_name = "DIR")]
    source: Option<PathBuf>,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "io.github.tnuproject.MetaUpd1.Error")]
enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    Busy(String),
    NotAuthorized(String),
    NoTrust(String),
    Verification(String),
    Incompatible(String),
    Failed(String),
}

fn convert(error: EngineError) -> Error {
    let text = error.to_string();
    match error {
        EngineError::TrustStore(metaupd::trust_store::TrustStoreError::NoAnchor(_)) => {
            Error::NoTrust(text)
        }
        EngineError::TrustStore(_)
        | EngineError::Trust(_)
        | EngineError::Hash(_)
        | EngineError::Sequence(_) => Error::Verification(text),
        EngineError::Incompatible(_) => Error::Incompatible(text),
        EngineError::Store(metaupd::store::StoreError::Locked) => Error::Busy(text),
        _ => Error::Failed(text),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Activity {
    Idle,
    Checking,
    Downloading,
    Preparing,
    RebootRequired,
    Failed,
}

impl Activity {
    fn as_str(self) -> &'static str {
        match self {
            Activity::Idle => "Idle",
            Activity::Checking => "Checking",
            Activity::Downloading => "Downloading",
            Activity::Preparing => "Preparing",
            Activity::RebootRequired => "RebootRequired",
            Activity::Failed => "Failed",
        }
    }
}

#[derive(Debug, Clone)]
struct Available {
    version: String,
    codename: String,
    channel: String,
    size: u64,
    reboot: bool,
    incompatibilities: Vec<String>,
}

struct Service {
    paths: Paths,
    source: Option<PathBuf>,
    activity: Mutex<Activity>,
    busy: Mutex<bool>,
    available: Mutex<Option<Available>>,
    next_job: AtomicU32,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Service {
    /// Runs `work` with a freshly configured engine.
    fn with_engine<T>(
        &self,
        work: impl FnOnce(&Engine<'_>) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let runner = SystemRunner;
        let source: Box<dyn ReleaseSource> = match &self.source {
            Some(directory) => Box::new(DirectorySource::new(directory)),
            None => Box::new(GithubSource::official(CommandFetcher)),
        };
        let (host, edition) = host_from_platform(&Platform::detect(&runner));
        work(&Engine {
            paths: self.paths.clone(),
            source: source.as_ref(),
            runner: &runner,
            now: Timestamp::now(),
            host,
            edition,
            require_separate_state: true,
        })
    }

    fn check(&self) -> Result<String, EngineError> {
        let report = self.with_engine(|engine| engine.check())?;
        let available = report.offer.as_ref().map(|offer| {
            let manifest = offer.manifest.manifest();
            let artifacts: Vec<_> = manifest.artifacts.iter().collect();
            Available {
                version: manifest.version.to_string(),
                codename: manifest.codename.clone(),
                channel: manifest.channel.to_string(),
                size: artifacts.iter().map(|a| a.size).sum(),
                reboot: artifacts.iter().any(|a| a.requires_reboot),
                incompatibilities: offer
                    .incompatibilities
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            }
        });
        let mut message = match &available {
            Some(found) => format!("META {} is available", found.version),
            None => "no update is available".to_owned(),
        };
        for warning in &report.warnings {
            message.push_str(&format!("\nWarning: {warning}"));
        }
        for (tag, reason) in &report.rejected {
            message.push_str(&format!("\nRejected {tag}: {reason}"));
        }
        *lock(&self.available) = available;
        Ok(message)
    }

    fn download(&self, version: &str) -> Result<String, EngineError> {
        let (offer, _) = self.with_engine(|engine| engine.download())?;
        let found = offer.manifest.manifest().version.to_string();
        if found == version {
            Ok(format!("META {found} was downloaded and verified"))
        } else {
            Err(EngineError::Unsupported(format!(
                "META {found} was offered instead of {version}"
            )))
        }
    }

    fn install(&self, version: &str) -> Result<String, EngineError> {
        let pending = self.with_engine(|engine| {
            let staged = engine.staged(version)?;
            if !staged.incompatibilities.is_empty() {
                return Err(EngineError::Incompatible(staged.incompatibilities));
            }
            apply::apply(engine, &staged)
        })?;
        Ok(format!(
            "META {} is ready in {}",
            pending.version, pending.boot_environment
        ))
    }
}

#[derive(Clone)]
struct Updates(Arc<Service>);

async fn authorize(
    connection: &Connection,
    header: &Header<'_>,
    action: &str,
) -> Result<(), Error> {
    let action = format!("io.github.tnuproject.metaupd.{action}");
    meta_dbus::authorize(connection, header, &action)
        .await
        .map_err(Error::NotAuthorized)
}

impl Updates {
    /// Starts a job, which reports through its own object.
    async fn start(
        &self,
        connection: &Connection,
        kind: &'static str,
        activity: Activity,
        work: impl FnOnce(&Service) -> Result<String, EngineError> + Send + 'static,
    ) -> Result<OwnedObjectPath, Error> {
        {
            let mut busy = lock(&self.0.busy);
            if *busy {
                return Err(Error::Busy("another operation is running".into()));
            }
            *busy = true;
        }
        let number = self.0.next_job.fetch_add(1, Ordering::Relaxed);
        let path = format!("{PATH}/jobs/{number}");
        let started = connection
            .object_server()
            .at(path.as_str(), Job { kind })
            .await;
        if let Err(error) = started {
            *lock(&self.0.busy) = false;
            return Err(error.into());
        }
        self.enter(connection, activity).await;
        let this = self.clone();
        let connection = connection.clone();
        let job = path.clone();
        std::thread::spawn(move || {
            // A panic must not leave the service busy forever.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&this.0)))
                .unwrap_or_else(|_| Err(EngineError::Unsupported("internal error".into())));
            zbus::block_on(this.finish(&connection, &job, activity, result));
        });
        OwnedObjectPath::try_from(path).map_err(|e| Error::Failed(e.to_string()))
    }

    async fn enter(&self, connection: &Connection, activity: Activity) {
        *lock(&self.0.activity) = activity;
        if let Ok(object) = connection
            .object_server()
            .interface::<_, Updates>(PATH)
            .await
        {
            let _ = object
                .get()
                .await
                .state_changed(object.signal_emitter())
                .await;
        }
    }

    async fn finish(
        &self,
        connection: &Connection,
        job: &str,
        activity: Activity,
        result: Result<String, EngineError>,
    ) {
        let next = match (&result, activity) {
            (Err(_), _) => Activity::Failed,
            (Ok(_), Activity::Preparing) => Activity::RebootRequired,
            (Ok(_), _) => Activity::Idle,
        };
        *lock(&self.0.busy) = false;
        self.enter(connection, next).await;
        let (success, message) = match result {
            Ok(message) => (true, message),
            Err(error) => (false, error.to_string()),
        };
        let server = connection.object_server();
        if let Ok(object) = server.interface::<_, Updates>(PATH).await {
            let emitter = object.signal_emitter();
            let _ = object.get().await.last_check_changed(emitter).await;
            if activity == Activity::Checking {
                let _ = Updates::available_changed(emitter).await;
            }
            if next == Activity::RebootRequired {
                let _ = Updates::reboot_required(emitter).await;
            }
        }
        if let Ok(object) = server.interface::<_, Job>(job).await {
            let _ = Job::finished(object.signal_emitter(), success, &message).await;
        }
        let _ = server.remove::<Job, _>(job).await;
    }
}

#[interface(name = "io.github.tnuproject.MetaUpd1")]
impl Updates {
    async fn check(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<OwnedObjectPath, Error> {
        authorize(connection, &header, "check").await?;
        self.start(connection, "check", Activity::Checking, Service::check)
            .await
    }

    async fn download(
        &self,
        version: String,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<OwnedObjectPath, Error> {
        authorize(connection, &header, "download").await?;
        let work = move |service: &Service| service.download(&version);
        self.start(connection, "download", Activity::Downloading, work)
            .await
    }

    async fn install(
        &self,
        version: String,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<OwnedObjectPath, Error> {
        authorize(connection, &header, "install").await?;
        let work = move |service: &Service| service.install(&version);
        self.start(connection, "install", Activity::Preparing, work)
            .await
    }

    async fn set_channel(
        &self,
        channel: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), Error> {
        authorize(connection, &header, "set-channel").await?;
        let channel: Channel = channel.parse().map_err(Error::Failed)?;
        self.0
            .with_engine(|engine| engine.set_channel(channel))
            .map_err(convert)
    }

    fn get_available(&self) -> HashMap<String, OwnedValue> {
        let value = |value: Value<'_>| OwnedValue::try_from(value).ok();
        let Some(found) = lock(&self.0.available).clone() else {
            return HashMap::new();
        };
        [
            ("version", value(found.version.into())),
            ("codename", value(found.codename.into())),
            ("channel", value(found.channel.into())),
            ("size", value(found.size.into())),
            ("reboot", value(found.reboot.into())),
            ("incompatibilities", value(found.incompatibilities.into())),
        ]
        .into_iter()
        .filter_map(|(name, value)| Some((name.to_owned(), value?)))
        .collect()
    }

    fn get_history(&self) -> Result<Vec<(u64, String, bool, String)>, Error> {
        let history = history::load(&self.0.paths).map_err(|e| convert(EngineError::Store(e)))?;
        Ok(history
            .records
            .into_iter()
            .map(|r| {
                (
                    r.time.unix_seconds(),
                    r.action.as_str().to_owned(),
                    r.success,
                    r.detail,
                )
            })
            .collect())
    }

    fn list_boot_environments(&self) -> Result<Vec<(String, bool, bool, String)>, Error> {
        let list = metaupd_rollback::list(&SystemRunner).map_err(|e| convert(e.into()))?;
        Ok(list
            .into_iter()
            .map(|be| {
                (
                    be.name,
                    be.active_now,
                    be.active_on_reboot || be.active_once,
                    be.created,
                )
            })
            .collect())
    }

    async fn rollback(
        &self,
        name: &str,
        once: bool,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), Error> {
        authorize(connection, &header, "rollback").await?;
        self.0
            .with_engine(|engine| {
                let environments = engine.boot_environments()?;
                metaupd_rollback::choose_target(&environments, Some(name))?;
                engine.rollback(name, once)
            })
            .map_err(convert)?;
        self.enter(connection, Activity::RebootRequired).await;
        Ok(())
    }

    #[zbus(signal)]
    async fn available_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn reboot_required(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(property)]
    fn version(&self) -> String {
        METAUPD_VERSION.to_owned()
    }

    #[zbus(property)]
    fn installed_version(&self) -> String {
        Platform::detect(&SystemRunner)
            .meta_release
            .map(|r| r.version)
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn channel(&self) -> String {
        State::load(&self.0.paths)
            .map(|s| s.channel.to_string())
            .unwrap_or_default()
    }

    #[zbus(property)]
    fn last_check(&self) -> u64 {
        State::load(&self.0.paths)
            .ok()
            .and_then(|state| state.last_check)
            .map_or(0, Timestamp::unix_seconds)
    }

    #[zbus(property)]
    fn state(&self) -> String {
        let current = *lock(&self.0.activity);
        // Survives a restart of the service.
        let waiting = State::load(&self.0.paths).is_ok_and(|state| state.pending.is_some());
        if current == Activity::Idle && waiting {
            return Activity::RebootRequired.as_str().to_owned();
        }
        current.as_str().to_owned()
    }
}

struct Job {
    kind: &'static str,
}

#[interface(name = "io.github.tnuproject.MetaUpd1.Job")]
impl Job {
    #[zbus(property)]
    fn kind(&self) -> String {
        self.kind.to_owned()
    }

    /// -1: unknown.
    #[zbus(property)]
    fn progress(&self) -> i32 {
        -1
    }

    #[zbus(signal)]
    async fn finished(
        emitter: &SignalEmitter<'_>,
        success: bool,
        message: &str,
    ) -> zbus::Result<()>;
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("metaupdd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    Platform::detect(&SystemRunner).require_freebsd("metaupdd")?;
    let mut paths = Paths::default();
    if let Some(directory) = cli.state_dir {
        paths.state_dir = directory;
    }
    if let Some(directory) = cli.trust_dir {
        paths.trust_dir = directory;
    }
    let service = Updates(Arc::new(Service {
        paths,
        source: cli.source,
        activity: Mutex::new(Activity::Idle),
        busy: Mutex::new(false),
        available: Mutex::new(None),
        next_job: AtomicU32::new(1),
    }));
    let _connection = zbus::block_on(async {
        let connection = Connection::system().await?;
        connection.object_server().at(PATH, service).await?;
        connection
            .request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
            .await?;
        Ok::<_, zbus::Error>(connection)
    })
    .map_err(|e| match e {
        zbus::Error::NameTaken => format!("{NAME} is already running"),
        error => error.to_string(),
    })?;
    // The connection serves requests on its own threads.
    loop {
        std::thread::park();
    }
}
