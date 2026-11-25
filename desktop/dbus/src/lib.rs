//! Authorization of D-Bus callers.
//!
//! A caller is identified by its unique bus name, which the bus assigns and the
//! caller cannot choose. Nothing in a method's arguments affects who is asking.

use std::collections::HashMap;

use zbus::Connection;
use zbus::message::Header;
use zbus::names::BusName;
use zbus::zvariant::Value;

#[zbus::proxy(
    interface = "org.freedesktop.PolicyKit1.Authority",
    default_service = "org.freedesktop.PolicyKit1",
    default_path = "/org/freedesktop/PolicyKit1/Authority"
)]
trait Authority {
    #[zbus(allow_interactive_auth)]
    fn check_authorization(
        &self,
        subject: &(&str, HashMap<&str, Value<'_>>),
        action_id: &str,
        details: &HashMap<&str, &str>,
        flags: u32,
        cancellation_id: &str,
    ) -> zbus::Result<(bool, bool, HashMap<String, String>)>;
}

/// Lets polkit ask the user.
const ALLOW_USER_INTERACTION: u32 = 1;

/// Succeeds when the sender of `header` may perform `action`.
///
/// Root is always allowed, so that a system without polkit can be administered.
/// Any failure to get an answer from polkit counts as a refusal.
pub async fn authorize(
    connection: &Connection,
    header: &Header<'_>,
    action: &str,
) -> Result<(), String> {
    let sender = header.sender().ok_or("the caller cannot be identified")?;
    let bus = zbus::fdo::DBusProxy::new(connection)
        .await
        .map_err(|e| e.to_string())?;
    let uid = bus
        .get_connection_unix_user(BusName::Unique(sender.as_ref()))
        .await
        .map_err(|e| format!("the caller cannot be identified: {e}"))?;
    if uid == 0 {
        return Ok(());
    }
    let authority = AuthorityProxy::new(connection)
        .await
        .map_err(|e| e.to_string())?;
    let subject = (
        "system-bus-name",
        HashMap::from([("name", Value::from(sender.as_str()))]),
    );
    let answer = authority
        .check_authorization(
            &subject,
            action,
            &HashMap::new(),
            ALLOW_USER_INTERACTION,
            "",
        )
        .await;
    match answer {
        Ok((true, _, _)) => Ok(()),
        Ok((false, _, _)) => Err(format!("not authorized for {action}")),
        Err(error) => Err(format!("cannot ask polkit: {error}")),
    }
}
