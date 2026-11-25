//! Desktop environments, from desktop/desktops.toml.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Desktop {
    pub id: String,
    pub name: String,
    pub display: String,
    pub packages: Vec<String>,
    pub availability: Availability,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Availability {
    pub date: String,
    pub repository: String,
    pub available: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema: u32,
    desktop: Vec<Desktop>,
}

pub fn desktops() -> Vec<Desktop> {
    let catalog: Catalog = toml::from_str(include_str!("../../desktop/desktops.toml"))
        .expect("the catalog is validated by the test suite");
    assert_eq!(
        catalog.schema, 1,
        "the catalog is validated by the test suite"
    );
    catalog.desktop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_every_planned_desktop() {
        let ids: Vec<String> = desktops().into_iter().map(|d| d.id).collect();
        assert_eq!(
            ids,
            ["kde", "gnome", "xfce", "cinnamon", "i3", "sway", "hyprland"]
        );
    }
}
