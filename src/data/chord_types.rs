use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Change,
    Delete,
    Read,
    Insert,
    Move,
    Select,
    Yank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Positional {
    In,
    At,
    Around,
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Function,
    Variable,
    Block,
    Line,
    File,
    Struct,
    Impl,
    Enum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Component {
    Body,
    Name,
    Signature,
    Parameters,
    Type,
    Value,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordSpec {
    pub action: Action,
    pub positional: Positional,
    pub scope: Scope,
    pub component: Component,
    pub requires_lsp: bool,
}

impl ChordSpec {
    pub fn short_form(&self) -> String {
        format!(
            "{}{}{}{}",
            self.action.short(),
            self.positional.short(),
            self.scope.short(),
            self.component.short(),
        )
    }

    pub fn long_form(&self) -> String {
        format!(
            "{}{}{}{}",
            self.action, self.positional, self.scope, self.component,
        )
    }
}

impl Action {
    pub fn short(&self) -> &'static str {
        match self {
            Self::Change => "c",
            Self::Delete => "d",
            Self::Read => "r",
            Self::Insert => "i",
            Self::Move => "m",
            Self::Select => "s",
            Self::Yank => "y",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "c" => Some(Self::Change),
            "d" => Some(Self::Delete),
            "r" => Some(Self::Read),
            "i" => Some(Self::Insert),
            "m" => Some(Self::Move),
            "s" => Some(Self::Select),
            "y" => Some(Self::Yank),
            _ => None,
        }
    }
}

impl Positional {
    pub fn short(&self) -> &'static str {
        match self {
            Self::In => "i",
            Self::At => "a",
            Self::Around => "r",
            Self::Before => "b",
            Self::After => "f",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "i" => Some(Self::In),
            "a" => Some(Self::At),
            "r" => Some(Self::Around),
            "b" => Some(Self::Before),
            "f" => Some(Self::After),
            _ => None,
        }
    }
}

impl Scope {
    pub fn short(&self) -> &'static str {
        match self {
            Self::Function => "f",
            Self::Variable => "v",
            Self::Block => "b",
            Self::Line => "l",
            Self::File => "F",
            Self::Struct => "s",
            Self::Impl => "m",
            Self::Enum => "e",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "f" => Some(Self::Function),
            "v" => Some(Self::Variable),
            "b" => Some(Self::Block),
            "l" => Some(Self::Line),
            "F" => Some(Self::File),
            "s" => Some(Self::Struct),
            "m" => Some(Self::Impl),
            "e" => Some(Self::Enum),
            _ => None,
        }
    }

    pub fn requires_lsp(&self) -> bool {
        !matches!(self, Self::Line | Self::File)
    }
}

impl Component {
    pub fn short(&self) -> &'static str {
        match self {
            Self::Body => "b",
            Self::Name => "n",
            Self::Signature => "s",
            Self::Parameters => "p",
            Self::Type => "t",
            Self::Value => "v",
            Self::All => "a",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "b" => Some(Self::Body),
            "n" => Some(Self::Name),
            "s" => Some(Self::Signature),
            "p" => Some(Self::Parameters),
            "t" => Some(Self::Type),
            "v" => Some(Self::Value),
            "a" => Some(Self::All),
            _ => None,
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Change => write!(f, "Change"),
            Self::Delete => write!(f, "Delete"),
            Self::Read => write!(f, "Read"),
            Self::Insert => write!(f, "Insert"),
            Self::Move => write!(f, "Move"),
            Self::Select => write!(f, "Select"),
            Self::Yank => write!(f, "Yank"),
        }
    }
}

impl fmt::Display for Positional {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::In => write!(f, "In"),
            Self::At => write!(f, "At"),
            Self::Around => write!(f, "Around"),
            Self::Before => write!(f, "Before"),
            Self::After => write!(f, "After"),
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Function => write!(f, "Function"),
            Self::Variable => write!(f, "Variable"),
            Self::Block => write!(f, "Block"),
            Self::Line => write!(f, "Line"),
            Self::File => write!(f, "File"),
            Self::Struct => write!(f, "Struct"),
            Self::Impl => write!(f, "Impl"),
            Self::Enum => write!(f, "Enum"),
        }
    }
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Body => write!(f, "Body"),
            Self::Name => write!(f, "Name"),
            Self::Signature => write!(f, "Signature"),
            Self::Parameters => write!(f, "Parameters"),
            Self::Type => write!(f, "Type"),
            Self::Value => write!(f, "Value"),
            Self::All => write!(f, "All"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_form_round_trip() {
        let spec = ChordSpec {
            action: Action::Change,
            positional: Positional::In,
            scope: Scope::Function,
            component: Component::Body,
            requires_lsp: true,
        };
        assert_eq!(spec.short_form(), "cifb");
        assert_eq!(spec.long_form(), "ChangeInFunctionBody");
    }

    #[test]
    fn action_short_round_trip() {
        for action in [
            Action::Change,
            Action::Delete,
            Action::Read,
            Action::Insert,
            Action::Move,
            Action::Select,
            Action::Yank,
        ] {
            let short = action.short();
            assert_eq!(Action::from_short(short), Some(action));
        }
    }

    #[test]
    fn scope_lsp_requirement() {
        assert!(Scope::Function.requires_lsp());
        assert!(Scope::Variable.requires_lsp());
        assert!(!Scope::Line.requires_lsp());
        assert!(!Scope::File.requires_lsp());
    }

    #[test]
    fn delete_at_variable_value_short_form() {
        let spec = ChordSpec {
            action: Action::Delete,
            positional: Positional::At,
            scope: Scope::Variable,
            component: Component::Value,
            requires_lsp: true,
        };
        assert_eq!(spec.short_form(), "davv");
        assert_eq!(spec.long_form(), "DeleteAtVariableValue");
    }
}
