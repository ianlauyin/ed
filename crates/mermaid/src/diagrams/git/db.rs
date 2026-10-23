//! Builds a git graph's commits and branches from its syntax tree, as Mermaid's git graph
//! database does. Port of Pele's `src/diagrams/git/db.ts`.

use std::collections::BTreeMap;

use super::parser::{CommitTypeName, GitGraphAst, StatementAst};
use crate::error::{Code, Error};
use crate::util::chars::is_space;
use crate::yaml::Value;

/// A plain commit.
pub(crate) const NORMAL: u8 = 0;
/// A commit that reverses another.
pub(crate) const REVERSE: u8 = 1;
/// A highlighted commit.
pub(crate) const HIGHLIGHT: u8 = 2;
/// A merge commit.
pub(crate) const MERGE: u8 = 3;
/// A cherry-picked commit.
pub(crate) const CHERRY_PICK: u8 = 4;

/// A commit.
#[derive(Clone, Debug)]
pub(crate) struct GitCommit {
    /// The branch it was made on.
    pub(crate) branch: String,
    /// For a merge, whether its id was written.
    pub(crate) custom_id: Option<bool>,
    /// For a merge, the type written for it.
    pub(crate) custom_type: Option<u8>,
    /// The id.
    pub(crate) id: String,
    /// The message.
    pub(crate) message: String,
    /// Parent ids: the previous head first.
    pub(crate) parents: Vec<String>,
    /// Creation order.
    pub(crate) seq: u32,
    /// Tags, in written order.
    pub(crate) tags: Vec<String>,
    /// One of [`NORMAL`], [`REVERSE`], [`HIGHLIGHT`], [`MERGE`], and [`CHERRY_PICK`].
    pub(crate) r#type: u8,
}

/// A built git graph: Pele's `GitModel`.
#[derive(Clone, Debug, Default)]
pub(crate) struct GitModel {
    /// The accessible description.
    pub(crate) acc_descr: Option<String>,
    /// The accessible title.
    pub(crate) acc_title: Option<String>,
    /// Each branch and the id of its latest commit, or `None` while it has none, in creation
    /// order.
    pub(crate) branches: Vec<(String, Option<String>)>,
    /// Commits keyed by id, in the order the ids first appeared. A later commit with the same id
    /// replaces the earlier one in its place, as `Map.set` does.
    pub(crate) commits: Vec<GitCommit>,
    /// The checked-out branch.
    pub(crate) current_branch: String,
    /// `LR`, `TB`, or `BT`.
    pub(crate) direction: &'static str,
    /// The id of the head commit.
    pub(crate) head: Option<String>,
    /// Branch names in drawing order: by `order`, then by creation.
    pub(crate) lanes: Vec<String>,
    /// The title.
    pub(crate) title: Option<String>,
    /// Warnings Mermaid logs.
    pub(crate) warnings: Vec<String>,
}

/// A semantic error naming the git graph.
#[cold]
fn fail(message: String) -> Error {
    let mut error = Error::new(Code::Semantic, message);
    error.r#type = Some("gitGraph");
    error
}

/// `a + b + c` as one string.
fn join(parts: &[&str]) -> String {
    parts.concat()
}

fn type_code(name: CommitTypeName) -> u8 {
    match name {
        CommitTypeName::Highlight => HIGHLIGHT,
        CommitTypeName::Normal => NORMAL,
        CommitTypeName::Reverse => REVERSE,
    }
}

/// A property of a YAML mapping.
fn property<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    let Value::Object(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(own, _)| own == key)
        .map(|(_, value)| value)
}

/// Appends `value` in lowercase hexadecimal, padded to eight digits.
fn push_hex8(out: &mut String, value: u32) {
    for shift in (0..8).rev() {
        let digit = (value >> (shift * 4)) & 15;
        out.push(char::from_digit(digit, 16).unwrap_or('0'));
    }
}

/// The state of a build: Pele's closure variables.
struct Builder {
    branches: Vec<(String, Option<String>)>,
    commits: Vec<GitCommit>,
    current: String,
    direction: &'static str,
    head: Option<String>,
    index: BTreeMap<String, usize>,
    orders: Vec<(String, f64)>,
    seq: u32,
    warnings: Vec<String>,
}

impl Builder {
    fn has(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    fn get(&self, id: &str) -> Option<&GitCommit> {
        self.index.get(id).and_then(|&at| self.commits.get(at))
    }

    fn branch(&self, name: &str) -> Option<&Option<String>> {
        self.branches
            .iter()
            .find(|(own, _)| own == name)
            .map(|(_, head)| head)
    }

    fn set_branch(&mut self, name: &str, head: Option<String>) {
        if let Some(entry) = self.branches.iter_mut().find(|(own, _)| own == name) {
            entry.1 = head;
        } else {
            self.branches.push((name.to_owned(), head));
        }
    }

    /// Mermaid appends seven random hex digits to the sequence number. These digits are derived
    /// from the sequence number instead, so the same text always gives the same ids.
    fn next_id(&self) -> String {
        let mut salt = self.seq.wrapping_add(1);
        loop {
            let mut id = String::new();
            crate::error::push_u32(&mut id, self.seq);
            id.push('-');
            let mut hex = String::new();
            push_hex8(&mut hex, salt.wrapping_mul(0x9e37_79b1));
            id.push_str(hex.get(..7).unwrap_or(""));
            if !self.has(&id) {
                return id;
            }
            salt = salt.wrapping_add(0x9e37_79b1);
        }
    }

    fn add(&mut self, commit: GitCommit) {
        self.head = Some(commit.id.clone());
        let current = self.current.clone();
        self.set_branch(&current, Some(commit.id.clone()));
        if let Some(&at) = self.index.get(&commit.id)
            && let Some(slot) = self.commits.get_mut(at)
        {
            *slot = commit;
            return;
        }
        self.index.insert(commit.id.clone(), self.commits.len());
        self.commits.push(commit);
    }

    fn checkout(&mut self, branch: &str) -> Result<(), Error> {
        let Some(id) = self.branch(branch) else {
            return Err(fail(join(&[
                "Trying to checkout branch which is not yet created. (Help try using \"branch ",
                branch,
                "\")",
            ])));
        };
        let head = id
            .as_deref()
            .filter(|id| !id.is_empty())
            .and_then(|id| self.get(id))
            .map(|commit| commit.id.clone());
        branch.clone_into(&mut self.current);
        self.head = head;
        Ok(())
    }

    fn commit(&mut self, id: Option<&str>, msg: &str, r#type: u8, tags: &[String]) {
        let commit = GitCommit {
            branch: self.current.clone(),
            custom_id: None,
            custom_type: None,
            id: match id.filter(|id| !id.is_empty()) {
                Some(id) => id.to_owned(),
                None => self.next_id(),
            },
            message: msg.to_owned(),
            parents: self.head.iter().cloned().collect(),
            seq: self.seq,
            tags: tags.to_vec(),
            r#type,
        };
        self.seq += 1;
        if self.has(&commit.id) {
            self.warnings
                .push(join(&["Commit ID ", &commit.id, " already exists"]));
        }
        self.add(commit);
    }

    fn branch_statement(&mut self, name: &str, order: f64) -> Result<(), Error> {
        if self.branch(name).is_some() {
            return Err(fail(join(&[
                "Trying to create an existing branch. (Help: Either use a new name if you want create a new branch or try using \"checkout ",
                name,
                "\")",
            ])));
        }
        let head = self.head.clone();
        self.set_branch(name, head);
        self.orders.push((name.to_owned(), order));
        self.checkout(name)
    }

    fn merge(
        &mut self,
        other: &str,
        custom_id: &str,
        r#type: Option<u8>,
        tags: &[String],
    ) -> Result<(), Error> {
        let current = self.current.clone();
        let current_head = self.branch(&current).cloned().flatten();
        let other_head = self.branch(other).cloned().flatten();
        let current_commit = current_head
            .as_deref()
            .filter(|id| !id.is_empty())
            .and_then(|id| self.get(id));
        let other_commit = other_head
            .as_deref()
            .filter(|id| !id.is_empty())
            .and_then(|id| self.get(id));
        if let (Some(a), Some(_)) = (current_commit, other_commit)
            && a.branch == other
        {
            return Err(fail(join(&[
                "Cannot merge branch '",
                other,
                "' into itself.",
            ])));
        }
        if current == other {
            return Err(fail(String::from(
                "Incorrect usage of \"merge\". Cannot merge a branch to itself",
            )));
        }
        let Some(current_commit) = current_commit else {
            return Err(fail(join(&[
                "Incorrect usage of \"merge\". Current branch (",
                &current,
                ")has no commits",
            ])));
        };
        if self.branch(other).is_none() {
            return Err(fail(join(&[
                "Incorrect usage of \"merge\". Branch to be merged (",
                other,
                ") does not exist",
            ])));
        }
        let Some(other_commit) = other_commit else {
            return Err(fail(join(&[
                "Incorrect usage of \"merge\". Branch to be merged (",
                other,
                ") has no commits",
            ])));
        };
        // `currentCommit === otherCommit` compares the objects the map holds.
        if std::ptr::eq(current_commit, other_commit) {
            return Err(fail(String::from(
                "Incorrect usage of \"merge\". Both branches have same head",
            )));
        }
        if !custom_id.is_empty() && self.has(custom_id) {
            return Err(fail(join(&[
                "Incorrect usage of \"merge\". Commit with id:",
                custom_id,
                " already exists, use different custom id",
            ])));
        }

        let parents = match &self.head {
            Some(head) => vec![head.clone(), other_head.unwrap_or_default()],
            None => Vec::new(),
        };
        let message = join(&["merged branch ", other, " into ", &current]);
        let commit = GitCommit {
            branch: current,
            custom_id: Some(!custom_id.is_empty()),
            custom_type: r#type,
            id: if custom_id.is_empty() {
                self.next_id()
            } else {
                custom_id.to_owned()
            },
            message,
            parents,
            seq: self.seq,
            tags: tags.to_vec(),
            r#type: MERGE,
        };
        self.seq += 1;
        self.add(commit);
        Ok(())
    }

    fn cherry_pick(
        &mut self,
        source_id: Option<&str>,
        tags: Option<&[String]>,
        parent: Option<&str>,
    ) -> Result<(), Error> {
        let Some(source) = source_id
            .filter(|id| !id.is_empty())
            .and_then(|id| self.get(id))
        else {
            return Err(fail(String::from(
                "Incorrect usage of \"cherryPick\". Source commit id should exist and provided",
            )));
        };
        let parent = parent.filter(|parent| !parent.is_empty());
        if let Some(parent) = parent
            && !source.parents.iter().any(|own| own == parent)
        {
            return Err(fail(String::from(
                "Invalid operation: The specified parent commit is not an immediate parent of the cherry-picked commit.",
            )));
        }
        if source.r#type == MERGE && parent.is_none() {
            return Err(fail(String::from(
                "Incorrect usage of cherry-pick: If the source commit is a merge commit, an immediate parent commit must be specified.",
            )));
        }
        if source.branch == self.current {
            return Err(fail(String::from(
                "Incorrect usage of \"cherryPick\". Source commit is already on current branch",
            )));
        }
        let current_head = self.branch(&self.current).cloned().flatten();
        if !current_head
            .as_deref()
            .is_some_and(|id| !id.is_empty() && self.has(id))
        {
            return Err(fail(join(&[
                "Incorrect usage of \"cherry-pick\". Current branch (",
                &self.current,
                ")has no commits",
            ])));
        }

        let source_id = source.id.clone();
        let tags = if let Some(tags) = tags {
            tags.iter().filter(|tag| !tag.is_empty()).cloned().collect()
        } else {
            let mut tag = join(&["cherry-pick:", &source_id]);
            if source.r#type == MERGE {
                tag.push_str("|parent:");
                // A merge always has its parent here, so `undefined` never shows.
                tag.push_str(parent.unwrap_or("undefined"));
            }
            vec![tag]
        };
        let message = join(&["cherry-picked ", &source.message, " into ", &self.current]);
        let parents = match &self.head {
            Some(head) => vec![head.clone(), source_id],
            None => Vec::new(),
        };
        let commit = GitCommit {
            branch: self.current.clone(),
            custom_id: None,
            custom_type: None,
            id: self.next_id(),
            message,
            parents,
            seq: self.seq,
            tags,
            r#type: CHERRY_PICK,
        };
        self.seq += 1;
        self.add(commit);
        Ok(())
    }
}

/// `text.replace(/\n\s+/g, '\n')`.
fn squeeze_line_starts(text: &str) -> String {
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

/// Builds the model of `ast` with the `gitGraph` section of `config`. `title` comes from front
/// matter.
///
/// # Errors
///
/// Returns a semantic error for a statement Mermaid rejects, such as checking out a branch that
/// does not exist.
pub(crate) fn build_git(
    ast: GitGraphAst,
    config: &Value,
    title: Option<&str>,
) -> Result<GitModel, Error> {
    let git = property(config, "gitGraph");
    let main_name = match git.and_then(|git| property(git, "mainBranchName")) {
        Some(Value::String(name)) => name.clone(),
        _ => String::from("main"),
    };
    let main_order = match git.and_then(|git| property(git, "mainBranchOrder")) {
        Some(Value::Number(order)) => *order,
        _ => 0.0,
    };

    let mut b = Builder {
        branches: vec![(main_name.clone(), None)],
        commits: Vec::new(),
        current: main_name.clone(),
        direction: "LR",
        head: None,
        index: BTreeMap::new(),
        orders: vec![(main_name, main_order)],
        seq: 0,
        warnings: Vec::new(),
    };

    if let Some(dir) = ast.dir {
        b.direction = dir;
    }
    for statement in &ast.statements {
        match statement {
            StatementAst::Commit {
                id,
                message,
                tags,
                r#type,
            } => b.commit(
                id.as_deref(),
                message.as_deref().unwrap_or(""),
                r#type.map_or(NORMAL, type_code),
                tags,
            ),
            StatementAst::Branch { name, order } => {
                b.branch_statement(name, order.unwrap_or(0.0))?;
            }
            StatementAst::Merge {
                branch,
                id,
                tags,
                r#type,
            } => b.merge(
                branch,
                id.as_deref().unwrap_or(""),
                r#type.map(type_code),
                tags,
            )?,
            StatementAst::Checkout { branch } => b.checkout(branch)?,
            StatementAst::CherryPicking { id, parent, tags } => b.cherry_pick(
                id.as_deref(),
                (!tags.is_empty()).then_some(tags.as_slice()),
                parent.as_deref(),
            )?,
        }
    }

    // A branch made without an order sorts by creation among those of order zero, as in Mermaid.
    // `a.order - b.order` is NaN for a NaN order, which JavaScript reads as equal; this reads NaN
    // as zero so the order stays total.
    let mut orders = b.orders;
    orders.sort_by(|a, b| {
        let key = |order: f64| if order.is_nan() { 0.0 } else { order };
        key(a.1)
            .partial_cmp(&key(b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let lanes = orders.into_iter().map(|(name, _)| name).collect();

    Ok(GitModel {
        acc_descr: ast
            .acc_descr
            .filter(|text| !text.is_empty())
            .map(|text| squeeze_line_starts(&text)),
        acc_title: ast
            .acc_title
            .filter(|text| !text.is_empty())
            .map(|text| crate::text::label::trim_start(&text).to_owned()),
        branches: b.branches,
        commits: b.commits,
        current_branch: b.current,
        direction: b.direction,
        head: b.head,
        lanes,
        title: match ast.title.filter(|title| !title.is_empty()) {
            Some(title) => Some(title),
            None => title.map(str::to_owned),
        },
        warnings: b.warnings,
    })
}
