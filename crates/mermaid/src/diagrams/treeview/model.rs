//! The tree view model: entries nested by indentation under a root folder. Port of Pele's
//! `src/diagrams/treeview/model.ts`.

use super::box_drawing::{preprocess_box_drawing, remap_error_lines};
use super::parser::{TreeViewAst, parse_tree_view};
use crate::error::Error;
use crate::text::label::trim_start;
use crate::util::chars::is_space;

/// Whether an entry is a file or a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeType {
    /// A folder: a name written with a trailing `/`.
    Directory,
    /// A file.
    File,
}

impl NodeType {
    /// The type as Pele names it.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
        }
    }
}

/// An entry: Pele's `TreeNode`, with its children as indices into [`TreeViewModel::nodes`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeNode {
    /// Child entries, in authored order.
    pub(crate) children: Vec<usize>,
    /// The `:::` classes.
    pub(crate) css_class: Option<String>,
    /// The `##` description.
    pub(crate) description: Option<String>,
    /// An icon name as written (`pack:name`, `file`, `folder`), or `none` to hide the icon.
    pub(crate) icon: Option<String>,
    /// The order in which the entry was added, the root being 0.
    pub(crate) id: u32,
    /// The indentation the entry was written at, or -1 for the root.
    pub(crate) level: i64,
    /// The name, without a folder's trailing `/`.
    pub(crate) name: String,
    /// Whether the entry is a file or a folder.
    pub(crate) node_type: NodeType,
}

/// The parsed tree view: Pele's `TreeViewModel`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeViewModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// The number of entries, the root included.
    pub(crate) count: u32,
    /// Every entry. The first is a folder named `/` at level -1 that holds the top-level entries.
    pub(crate) nodes: Vec<TreeNode>,
    /// The title.
    pub(crate) title: Option<String>,
}

impl TreeViewModel {
    /// The entry at `index`, when there is one.
    pub(crate) fn node(&self, index: usize) -> Option<&TreeNode> {
        self.nodes.get(index)
    }

    /// The top-level entries.
    pub(crate) fn roots(&self) -> &[usize] {
        self.nodes.first().map_or(&[], |root| &root.children)
    }
}

/// Mermaid's tree view database, filled from nodes in source order with their indentation.
struct TreeViewDb {
    /// Every entry, the root first.
    nodes: Vec<TreeNode>,
    /// The open folders, from the root down.
    stack: Vec<usize>,
}

impl TreeViewDb {
    /// A database holding only the root.
    fn new() -> Self {
        Self {
            nodes: vec![TreeNode {
                children: Vec::new(),
                css_class: None,
                description: None,
                icon: None,
                id: 0,
                level: -1,
                name: String::from("/"),
                node_type: NodeType::Directory,
            }],
            stack: vec![0],
        }
    }

    /// Adds `node` under the nearest open entry indented less than it.
    fn add_node(&mut self, mut node: TreeNode) {
        while let Some(&top) = self.stack.last()
            && self
                .nodes
                .get(top)
                .is_some_and(|open| node.level <= open.level)
        {
            self.stack.pop();
        }
        let index = self.nodes.len();
        node.id = u32::try_from(index).unwrap_or(u32::MAX);
        if let Some(parent) = self.stack.last().and_then(|&top| self.nodes.get_mut(top)) {
            parent.children.push(index);
        }
        self.nodes.push(node);
        self.stack.push(index);
    }
}

/// Reads the source to a syntax tree, accepting box-drawing input, as Pele's `readTreeView`
/// does. Errors name lines of the original text.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn read_tree_view(source: &str) -> Result<TreeViewAst, Error> {
    let pre = preprocess_box_drawing(source)?;
    parse_tree_view(&pre.text).map_err(|mut error| {
        if pre.line_map.is_empty() {
            return error;
        }
        error.message = match error.message.find('\n') {
            None => remap_error_lines(&error.message, &pre),
            Some(head) => {
                let mut message = remap_error_lines(error.message.get(..head).unwrap_or(""), &pre);
                message.push_str(error.message.get(head..).unwrap_or(""));
                message
            }
        };
        error.line = pre.original(error.line).unwrap_or(error.line);
        error
    })
}

/// `text.replace(/\n\s+/g, '\n')`.
fn dedent_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\n' {
            while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
        }
    }
    out
}

/// Builds the model from `ast`, as Pele's `buildTreeView` does. `title` comes from front matter.
pub(crate) fn build_tree_view(ast: TreeViewAst, title: Option<&str>) -> TreeViewModel {
    let mut db = TreeViewDb::new();
    for node in ast.nodes {
        let mut name = node.name;
        let directory = name.ends_with('/');
        if directory {
            name.pop();
        }
        db.add_node(TreeNode {
            children: Vec::new(),
            css_class: node.class_annotation.filter(|class| !class.is_empty()),
            description: node.desc_annotation.filter(|text| !text.is_empty()),
            icon: node.icon_annotation.map(|icon| {
                if icon.is_empty() {
                    String::from("none")
                } else {
                    icon
                }
            }),
            id: 0,
            level: node.indent.map_or(0, i64::from),
            name,
            node_type: if directory {
                NodeType::Directory
            } else {
                NodeType::File
            },
        });
    }

    let count = u32::try_from(db.nodes.len()).unwrap_or(u32::MAX);
    TreeViewModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| dedent_lines(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| trim_start(&text).to_owned()),
        count,
        nodes: db.nodes,
        title: ast
            .title
            .filter(|text| !text.is_empty())
            .or_else(|| title.map(str::to_owned)),
    }
}
