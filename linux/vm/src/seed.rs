//! First-boot configuration of a guest, for cloud-init.
//!
//! All values were validated to contain nothing that needs quoting in YAML.

use crate::config::{Cgroups, VmConfig};

/// Public and private material a guest starts with.
#[derive(Debug, Clone)]
pub struct Keys {
    /// Lets the owner in.
    pub client_public: String,
    /// The guest's own identity, chosen by the host so that it can be checked from
    /// the first connection on.
    pub host_private: String,
    pub host_public: String,
}

pub fn meta_data(config: &VmConfig) -> String {
    format!(
        "instance-id: {}\nlocal-hostname: {}\n",
        config.uuid, config.name
    )
}

pub fn user_data(config: &VmConfig, keys: &Keys) -> String {
    let private: String = keys
        .host_private
        .trim_end()
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect();
    let mut text = format!(
        "#cloud-config\n\
         hostname: {name}\n\
         ssh_pwauth: false\n\
         disable_root: true\n\
         ssh_deletekeys: true\n\
         ssh_keys:\n  ed25519_private: |\n{private}  ed25519_public: {public}\n\
         users:\n\
         \x20 - name: {owner}\n\
         \x20   uid: {uid}\n\
         \x20   shell: /bin/bash\n\
         \x20   lock_passwd: true\n\
         \x20   sudo: ALL=(ALL) NOPASSWD:ALL\n\
         \x20   ssh_authorized_keys:\n\
         \x20     - {client}\n",
        name = config.name,
        public = keys.host_public.trim(),
        owner = config.owner,
        uid = config.owner_uid,
        client = keys.client_public.trim(),
    );
    if !config.shares.is_empty() {
        // Linux attaches as "nobody" unless told otherwise; bhyve's server knows
        // only the users listed in its jail.
        text.push_str("mounts:\n");
        for share in &config.shares {
            let mode = if share.read_only { ",ro" } else { "" };
            text.push_str(&format!(
                "  - [ {tag}, /mnt/host/{tag}, 9p, \"trans=virtio,version=9p2000.L,uname=root,nofail,_netdev,nosuid,nodev{mode}\", \"0\", \"0\" ]\n",
                tag = share.tag
            ));
        }
    }
    if config.cgroups == Cgroups::V1 {
        // systemd chooses the hierarchy from the kernel command line.
        text.push_str(
            "write_files:\n\
             \x20 - path: /etc/default/grub.d/90-meta-cgroups.cfg\n\
             \x20   content: |\n\
             \x20     GRUB_CMDLINE_LINUX=\"$GRUB_CMDLINE_LINUX systemd.unified_cgroup_hierarchy=0 systemd.legacy_systemd_cgroup_controller=1\"\n\
             runcmd:\n\
             \x20 - [ update-grub ]\n\
             power_state:\n\
             \x20 mode: reboot\n\
             \x20 message: control group version\n\
             \x20 condition: true\n",
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::sample;

    fn keys() -> Keys {
        Keys {
            client_public: "ssh-ed25519 AAAA client\n".into(),
            host_private: "-----BEGIN X-----\nabc\n-----END X-----\n".into(),
            host_public: "ssh-ed25519 BBBB host\n".into(),
        }
    }

    #[test]
    fn guest_is_locked_down_and_knows_its_owner() {
        let text = user_data(&sample(), &keys());
        assert!(text.starts_with("#cloud-config\n"));
        for expected in [
            "ssh_pwauth: false",
            "disable_root: true",
            "lock_passwd: true",
            "  - name: alice",
            "    uid: 1001",
            "      - ssh-ed25519 AAAA client",
            "    abc\n",
            "/mnt/host/share, 9p",
            "nosuid,nodev",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        assert!(!text.contains("unified_cgroup_hierarchy"));
    }

    #[test]
    fn version_1_control_groups_are_requested_from_the_kernel() {
        let mut config = sample();
        config.cgroups = Cgroups::V1;
        let text = user_data(&config, &keys());
        assert!(text.contains("systemd.unified_cgroup_hierarchy=0"));
        assert!(text.contains("mode: reboot"));
    }

    #[test]
    fn read_only_shares_are_read_only_in_the_guest_too() {
        let mut config = sample();
        config.shares[0].read_only = true;
        assert!(user_data(&config, &keys()).contains("nodev,ro\""));
    }
}
