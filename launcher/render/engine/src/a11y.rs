//! Screen readers: the focus tree as an AccessKit tree, so Orca (AT-SPI on Linux) reads what has
//! focus and can move and activate like the pad does.
//!
//! The tree is built from what the screen already registers for controller focus (`focus::Nav`):
//! every focusable in the active scope becomes a button (or a list item inside a rail, grid or
//! list), labelled by its `label` (else the screen's text for it), with its A hint as the
//! description, under its group; screens add the text worth reading that is not focusable
//! (headings, a sheet's body, the hint bar). Focus follows the engine's focus, and a screen
//! reader's Focus and Click land on the same node as a d-pad move and an A.
//!
//! `build` is pure (tested below). `A11y` wraps AccessKit's winit adapter: create it before the
//! window is shown, pass it every window event, and call `update` after focus or the screen
//! changed. Ids: the root is 0, scene nodes are their taffy id + 1, groups sit at GROUP_BASE.

pub use accesskit;
use accesskit::{Action, ActionRequest, Node, NodeId, Rect, Role, TreeId, TreeInfo, TreeUpdate};

use crate::focus::Nav;
use crate::scene::Id;

pub const ROOT: NodeId = NodeId(0);
const GROUP_BASE: u64 = 1 << 62;
const TEXT_BASE: u64 = 1 << 61;

pub fn node_id(id: Id) -> NodeId {
    NodeId(u64::from(id) + 1)
}

/// The scene node behind an AccessKit id (None for the root, groups and texts).
pub fn scene_id(n: NodeId) -> Option<Id> {
    (n.0 >= 1 && n.0 < TEXT_BASE).then(|| Id::from(n.0 - 1))
}

/// Text a screen reader should reach that is not on a focusable.
#[derive(Clone, Debug)]
pub struct Text {
    pub rect: [f32; 4],
    pub text: String,
    /// A section or page heading.
    pub heading: bool,
}

fn bounds(r: &[f32; 4]) -> Rect {
    Rect { x0: r[0] as f64, y0: r[1] as f64, x1: (r[0] + r[2]) as f64, y1: (r[1] + r[3]) as f64 }
}

/// The whole tree for this frame. `label_of` gives a focusable's text when it has no `label`
/// (the screen knows what a tile or button says).
pub fn build(nav: &Nav, texts: &[Text], title: &str, size: [f32; 2], label_of: impl Fn(Id) -> Option<String>) -> TreeUpdate {
    let mut nodes: Vec<(NodeId, Node)> = Vec::new();
    let mut root = Node::new(Role::Window);
    root.set_label(title);
    root.set_bounds(bounds(&[0.0, 0.0, size[0], size[1]]));
    let mut groups: Vec<Option<Node>> = vec![None; nav.groups.len()];
    // What focus can reach now, plus disabled buttons in the same scope (read as unavailable).
    let focusable: Vec<usize> = (0..nav.items.len()).filter(|&i| nav.reachable(&nav.items[i], true)).collect();
    for &i in &focusable {
        let it = &nav.items[i];
        let id = node_id(it.id);
        let in_list = it.group.is_some_and(|g| nav.groups[g].rail || !nav.groups[g].remember);
        let mut n = Node::new(if in_list { Role::ListItem } else { Role::Button });
        if let Some(l) = it.label.clone().or_else(|| label_of(it.id)) {
            n.set_label(l);
        }
        if let Some(a) = &it.hint_a {
            n.set_description(a.clone());
        }
        n.set_bounds(bounds(&it.rect));
        if it.disabled {
            n.set_disabled();
        } else {
            n.add_action(Action::Focus);
            n.add_action(Action::Click);
        }
        match it.group {
            Some(g) => {
                let gn = groups[g].get_or_insert_with(|| {
                    let grp = &nav.groups[g];
                    let mut gn = Node::new(if grp.rail || !grp.remember { Role::List } else { Role::Group });
                    gn.set_bounds(bounds(&grp.rect));
                    gn
                });
                if gn.children().is_empty() {
                    root.push_child(NodeId(GROUP_BASE + g as u64));
                }
                gn.push_child(id);
            }
            None => root.push_child(id),
        }
        nodes.push((id, n));
    }
    for (g, gn) in groups.into_iter().enumerate() {
        if let Some(gn) = gn {
            nodes.push((NodeId(GROUP_BASE + g as u64), gn));
        }
    }
    for (k, t) in texts.iter().enumerate() {
        let id = NodeId(TEXT_BASE + k as u64);
        let mut n = Node::new(if t.heading { Role::Heading } else { Role::Label });
        n.set_value(t.text.clone());
        n.set_bounds(bounds(&t.rect));
        root.push_child(id);
        nodes.push((id, n));
    }
    nodes.insert(0, (ROOT, root));
    let focus = nav.current().map(node_id).unwrap_or(ROOT);
    let mut tree = TreeInfo::new(ROOT);
    tree.toolkit_name = Some("Rare Render".into());
    tree.toolkit_version = Some(env!("CARGO_PKG_VERSION").into());
    TreeUpdate { nodes, tree: Some(tree), tree_id: TreeId::ROOT, focus }
}

/// What a screen reader asked for, in the engine's terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Put focus here (as a d-pad move would).
    Focus(Id),
    /// Activate (as A would).
    Activate(Id),
}

pub fn request(r: &ActionRequest) -> Option<Request> {
    let id = scene_id(r.target_node)?;
    match r.action {
        Action::Focus => Some(Request::Focus(id)),
        Action::Click => Some(Request::Activate(id)),
        _ => None,
    }
}

pub use accesskit_winit::{Event, WindowEvent};

/// AccessKit's winit adapter for the launcher window.
pub struct A11y {
    adapter: accesskit_winit::Adapter,
}

impl A11y {
    /// Before the window is shown. Screen reader events come back through the event loop proxy
    /// as `Event`s: answer `InitialTreeRequested` with `update`, and route `ActionRequested`
    /// through `request`.
    pub fn new<T: From<Event> + Send + 'static>(event_loop: &winit::event_loop::ActiveEventLoop, window: &winit::window::Window, proxy: winit::event_loop::EventLoopProxy<T>) -> A11y {
        A11y { adapter: accesskit_winit::Adapter::with_event_loop_proxy(event_loop, window, proxy) }
    }

    /// Every window event (focus, resize, the window's position for bounds).
    pub fn window_event(&mut self, window: &winit::window::Window, event: &winit::event::WindowEvent) {
        self.adapter.process_event(window, event);
    }

    /// A new tree, built only while a screen reader is listening.
    pub fn update(&mut self, f: impl FnOnce() -> TreeUpdate) {
        self.adapter.update_if_active(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::{FocusOpts, Focusable, Group};
    use taffy::NodeId as T;

    fn nav() -> Nav {
        let mut n = Nav::new(1920.0, 1200.0);
        let strip = n.add_group(Group::new("strip", [0.0, 0.0, 800.0, 200.0]).rail());
        let hero = n.add_group(Group::new("hero", [0.0, 300.0, 400.0, 80.0]).remember());
        n.add(Focusable::new(T::from(1u64), [0.0, 0.0, 200.0, 200.0]).group(strip).hints(Some("Play"), None, None));
        n.add(Focusable::new(T::from(2u64), [220.0, 0.0, 200.0, 200.0]).group(strip));
        n.add(Focusable::new(T::from(3u64), [0.0, 300.0, 180.0, 80.0]).group(hero).label("Play"));
        n.add(Focusable::new(T::from(4u64), [200.0, 300.0, 180.0, 80.0]).group(hero).label("Details").disabled(true));
        n.add(Focusable::new(T::from(5u64), [1500.0, 300.0, 180.0, 80.0]).label("Sign in"));
        n
    }

    fn find(u: &TreeUpdate, id: NodeId) -> &Node {
        &u.nodes.iter().find(|(i, _)| *i == id).unwrap().1
    }

    #[test]
    fn the_tree_mirrors_focus_groups_and_labels() {
        let mut n = nav();
        n.focus(T::from(2u64), FocusOpts::default(), 0.0);
        let titles = |id: Id| Some(format!("Game {}", u64::from(id)));
        let u = build(&n, &[Text { rect: [0.0, 500.0, 300.0, 40.0], text: "Your Rare".into(), heading: true }], "Rare", [1920.0, 1200.0], titles);
        assert_eq!(u.focus, node_id(T::from(2u64)));
        assert_eq!(u.tree.as_ref().unwrap().root, ROOT);
        let root = find(&u, ROOT);
        assert_eq!(root.role(), Role::Window);
        // Strip (a list), hero (a group), the lone button, the heading.
        assert_eq!(root.children(), &[NodeId(GROUP_BASE), NodeId(GROUP_BASE + 1), node_id(T::from(5u64)), NodeId(TEXT_BASE)]);
        let strip = find(&u, NodeId(GROUP_BASE));
        assert_eq!(strip.role(), Role::List);
        assert_eq!(strip.children(), &[node_id(T::from(1u64)), node_id(T::from(2u64))]);
        assert_eq!(find(&u, NodeId(GROUP_BASE + 1)).role(), Role::Group);
        let tile = find(&u, node_id(T::from(1u64)));
        assert_eq!(tile.role(), Role::ListItem);
        assert_eq!(tile.label(), Some("Game 1"));
        assert_eq!(tile.description(), Some("Play"));
        assert!(tile.supports_action(Action::Click));
        let details = find(&u, node_id(T::from(4u64)));
        assert_eq!(details.label(), Some("Details"));
        assert!(details.is_disabled() && !details.supports_action(Action::Click));
        assert_eq!(find(&u, node_id(T::from(5u64))).role(), Role::Button);
        assert_eq!(find(&u, NodeId(TEXT_BASE)).value(), Some("Your Rare"));
        assert_eq!(find(&u, node_id(T::from(3u64))).bounds(), Some(Rect { x0: 0.0, y0: 300.0, x1: 180.0, y1: 380.0 }));
    }

    #[test]
    fn a_modal_scope_hides_the_page() {
        let mut n = nav();
        n.add(Focusable::new(T::from(50u64), [600.0, 600.0, 200.0, 60.0]).scope(1).label("Cancel"));
        let u = build(&n, &[], "Rare", [1920.0, 1200.0], |_| None);
        assert_eq!(find(&u, ROOT).children(), &[node_id(T::from(50u64))]);
        assert_eq!(u.focus, ROOT, "nothing focused yet");
    }

    #[test]
    fn screen_reader_actions_map_to_focus_and_a() {
        let r = |action, n| ActionRequest { action, target_tree: TreeId::ROOT, target_node: n, data: None };
        assert_eq!(request(&r(Action::Click, node_id(T::from(7u64)))), Some(Request::Activate(T::from(7u64))));
        assert_eq!(request(&r(Action::Focus, node_id(T::from(7u64)))), Some(Request::Focus(T::from(7u64))));
        assert_eq!(request(&r(Action::Click, ROOT)), None);
        assert_eq!(request(&r(Action::Click, NodeId(GROUP_BASE))), None);
        assert_eq!(request(&r(Action::ScrollDown, node_id(T::from(7u64)))), None);
    }
}
