//! Command palette items and ranking (⇧⌘P / ⇧⌘O): one field, mixed results, a "top result" that
//! wins regardless of category, and the shortcut shown on every row so users learn it.

use crate::shortcuts::{Shortcut, label};

/// What choosing a palette row does. Kept as data so the list can be built and ranked without UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    NewTab,
    CopyLastOutput,
    ToggleSidebar,
    SetUpTouchId,
    ManageSshKeys,
    OpenHost(String),
    SelectTab(u64),
    InsertCommand(String),
}

/// One row of the palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: &'static str,
    pub shortcut: Option<&'static str>,
    pub kind: String,
    pub action: PaletteAction,
}

impl PaletteItem {
    fn new(title: impl Into<String>, icon: &'static str, kind: impl Into<String>, action: PaletteAction) -> Self {
        PaletteItem { title: title.into(), subtitle: None, icon, shortcut: None, kind: kind.into(), action }
    }

    fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    fn shortcut(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
}

/// A host row's inputs.
pub struct PaletteHost<'a> {
    pub alias: &'a str,
    pub user: Option<&'a str>,
    pub host_name: Option<&'a str>,
    pub credential: &'a str,
}

/// A tab row's inputs.
pub struct PaletteTab<'a> {
    pub id: u64,
    pub title: &'a str,
    pub cwd: Option<&'a str>,
}

/// Every item, in the Swift app's order: actions, hosts, tabs, then up to 20 distinct recent commands.
pub fn all_items(hosts: &[PaletteHost<'_>], tabs: &[PaletteTab<'_>], recent: &[&str]) -> Vec<PaletteItem> {
    let mut items = vec![
        PaletteItem::new("New Tab", "+", "Action", PaletteAction::NewTab).shortcut(label(Shortcut::NewTab)),
        PaletteItem::new("Copy Last Output", "⧉", "Action", PaletteAction::CopyLastOutput).shortcut(label(Shortcut::CopyLastOutput)),
        PaletteItem::new("Toggle Sessions Sidebar", "▤", "Action", PaletteAction::ToggleSidebar).shortcut(label(Shortcut::ToggleSidebar)),
        PaletteItem::new("Set Up Touch ID for sudo…", "◉", "Security", PaletteAction::SetUpTouchId)
            .subtitle("Runs a visible, reviewable script"),
        PaletteItem::new("Manage SSH Keys and Hosts…", "⚿", "Security", PaletteAction::ManageSshKeys)
            .subtitle("Secure Enclave keys that ask for Touch ID"),
    ];
    items.extend(hosts.iter().map(|h| {
        let subtitle = [h.user, h.host_name].into_iter().flatten().collect::<Vec<_>>().join("@");
        PaletteItem::new(h.alias, "▣", format!("Host · {}", h.credential), PaletteAction::OpenHost(h.alias.into()))
            .subtitle(subtitle)
    }));
    items.extend(tabs.iter().map(|t| {
        let item = PaletteItem::new(t.title, ">_", "Tab", PaletteAction::SelectTab(t.id));
        match t.cwd {
            Some(cwd) => item.subtitle(cwd),
            None => item,
        }
    }));
    let mut seen = std::collections::HashSet::new();
    items.extend(
        recent
            .iter()
            .filter(|c| seen.insert(**c))
            .take(20)
            .map(|c| {
                PaletteItem::new(*c, "↺", "Recent", PaletteAction::InsertCommand((*c).into()))
                    .subtitle("Insert without running")
            }),
    );
    items
}

/// Ranks `items` for `query`: title prefix, then title contains, then subtitle contains (case-insensitive).
/// Items that match none are dropped; an empty query keeps everything in order.
pub fn rank(items: Vec<PaletteItem>, query: &str) -> Vec<PaletteItem> {
    if query.is_empty() {
        return items;
    }
    let q = query.to_lowercase();
    let mut scored: Vec<(PaletteItem, u8)> = items
        .into_iter()
        .filter_map(|item| {
            let title = item.title.to_lowercase();
            let score = if title.starts_with(&q) {
                0
            } else if title.contains(&q) {
                1
            } else if item.subtitle.as_ref().is_some_and(|s| s.to_lowercase().contains(&q)) {
                2
            } else {
                return None;
            };
            Some((item, score))
        })
        .collect();
    scored.sort_by_key(|(_, score)| *score);
    scored.into_iter().map(|(item, _)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(items: &[PaletteItem]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn rank_should_order_prefix_before_contains_before_subtitle() {
        let items = all_items(
            &[PaletteHost { alias: "tabby", user: None, host_name: Some("x.example"), credential: "SE" }],
            &[PaletteTab { id: 1, title: "build", cwd: Some("/tmp/tab") }],
            &[],
        );
        let ranked = rank(items, "tab");
        assert_eq!(titles(&ranked), ["tabby", "New Tab", "build"]);
    }

    #[test]
    fn rank_should_keep_everything_for_an_empty_query() {
        let items = all_items(&[], &[], &[]);
        assert_eq!(rank(items.clone(), ""), items);
    }

    #[test]
    fn all_items_should_deduplicate_recent_commands_and_cap_them_at_twenty() {
        let recent: Vec<String> = (0..30).map(|i| format!("cmd{i}")).chain(["cmd0".into()]).collect();
        let refs: Vec<&str> = recent.iter().map(String::as_str).collect();
        let recents = all_items(&[], &[], &refs).into_iter().filter(|i| i.kind == "Recent").count();
        assert_eq!(recents, 20);
    }

    #[test]
    fn host_subtitle_should_join_user_and_host_name() {
        let items = all_items(
            &[PaletteHost { alias: "a", user: Some("me"), host_name: Some("h"), credential: "Key" }],
            &[],
            &[],
        );
        assert_eq!(items[5].subtitle.as_deref(), Some("me@h"));
    }
}
