use std::{
    fmt,
    cell::RefCell,
    rc::Rc,
    collections::BTreeMap,
};
use azul_css::{ Css, CssDeclaration, CssProperty };
use {
    FastHashMap,
    id_tree::{Arena, NodeId},
    traits::Layout,
    dom::Dom,
    dom::NodeData,
    ui_state::UiState,
};

pub struct UiDescription<T: Layout> {
    pub(crate) ui_descr_arena: Rc<RefCell<Arena<NodeData<T>>>>,
    /// ID of the root node of the arena (usually NodeId(0))
    pub(crate) ui_descr_root: NodeId,
    /// This field is created from the Css
    pub(crate) styled_nodes: BTreeMap<NodeId, StyledNode>,
    /// In the display list, we take references to the `UiDescription.styled_nodes`
    ///
    /// However, if there is no style, we want to have a default style applied
    /// and the reference to that style has to live as least as long as the `self.styled_nodes`
    /// This is why we need this field here
    pub(crate) default_style_of_node: StyledNode,
    /// The style properties that should be overridden for this frame, cloned from the `Css`
    pub(crate) dynamic_style_overrides: BTreeMap<NodeId, FastHashMap<String, CssProperty>>,
}

impl<T: Layout> fmt::Debug for UiDescription<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UiDescription {{ \
            ui_descr_arena: {:?},
            ui_descr_root: {:?},
            styled_nodes: {:?},
            default_style_of_node: {:?},
            dynamic_style_overrides: {:?},
        }}",
            self.ui_descr_arena,
            self.ui_descr_root,
            self.styled_nodes,
            self.default_style_of_node,
            self.dynamic_style_overrides,
        )
    }
}

impl<T: Layout> Clone for UiDescription<T> {
    fn clone(&self) -> Self {
        Self {
            ui_descr_arena: self.ui_descr_arena.clone(),
            ui_descr_root: self.ui_descr_root,
            styled_nodes: self.styled_nodes.clone(),
            default_style_of_node: self.default_style_of_node.clone(),
            dynamic_style_overrides: self.dynamic_style_overrides.clone(),
        }
    }
}

impl<T: Layout> Default for UiDescription<T> {
    fn default() -> Self {
        use dom::NodeType;
        let default_dom = Dom::new(NodeType::Div);
        Self::from_dom(&UiState::from_dom(default_dom), &Css::default())
    }
}

impl<T: Layout> UiDescription<T> {
    /// Applies the styles to the nodes calculated from the `layout_screen`
    /// function and calculates the final display list that is submitted to the
    /// renderer.
    pub fn from_dom(ui_state: &UiState<T>, style: &Css) -> Self
    {
        ::style::match_dom_selectors(ui_state, &style)
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct StyledNode {
    /// The CSS constraints, after the cascading step
    pub(crate) style_constraints: Vec<CssDeclaration>,
}