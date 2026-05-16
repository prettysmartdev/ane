use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Change,
    Replace,
    Delete,
    Yank,
    Append,
    Prepend,
    Insert,
    Jump,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Positional {
    Inside,
    Until,
    After,
    Before,
    Next,
    Previous,
    Entire,
    Outside,
    To,
    Last,
    First,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Line,
    Buffer,
    Function,
    Variable,
    Struct,
    Member,
    Delimiter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Component {
    Beginning,
    Contents,
    End,
    Value,
    Parameters,
    Arguments,
    Name,
    Self_,
    Word,
    Definition,
}

impl Action {
    pub fn short(&self) -> &'static str {
        match self {
            Self::Change => "c",
            Self::Replace => "r",
            Self::Delete => "d",
            Self::Yank => "y",
            Self::Append => "a",
            Self::Prepend => "p",
            Self::Insert => "i",
            Self::Jump => "j",
            Self::List => "l",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "c" => Some(Self::Change),
            "r" => Some(Self::Replace),
            "d" => Some(Self::Delete),
            "y" => Some(Self::Yank),
            "a" => Some(Self::Append),
            "p" => Some(Self::Prepend),
            "i" => Some(Self::Insert),
            "j" => Some(Self::Jump),
            "l" => Some(Self::List),
            _ => None,
        }
    }

    pub fn requires_interactive(&self) -> bool {
        matches!(self, Self::Jump)
    }
}

impl Positional {
    pub fn short(&self) -> &'static str {
        match self {
            Self::Inside => "i",
            Self::Until => "u",
            Self::After => "a",
            Self::Before => "b",
            Self::Next => "n",
            Self::Previous => "p",
            Self::Entire => "e",
            Self::Outside => "o",
            Self::To => "t",
            Self::Last => "l",
            Self::First => "f",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "i" => Some(Self::Inside),
            "u" => Some(Self::Until),
            "a" => Some(Self::After),
            "b" => Some(Self::Before),
            "n" => Some(Self::Next),
            "p" => Some(Self::Previous),
            "e" => Some(Self::Entire),
            "o" => Some(Self::Outside),
            "t" => Some(Self::To),
            "l" => Some(Self::Last),
            "f" => Some(Self::First),
            _ => None,
        }
    }
}

impl Scope {
    pub fn short(&self) -> &'static str {
        match self {
            Self::Line => "l",
            Self::Buffer => "b",
            Self::Function => "f",
            Self::Variable => "v",
            Self::Struct => "s",
            Self::Member => "m",
            Self::Delimiter => "d",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "l" => Some(Self::Line),
            "b" => Some(Self::Buffer),
            "f" => Some(Self::Function),
            "v" => Some(Self::Variable),
            "s" => Some(Self::Struct),
            "m" => Some(Self::Member),
            "d" => Some(Self::Delimiter),
            _ => None,
        }
    }

    pub fn requires_lsp(&self) -> bool {
        !matches!(self, Self::Line | Self::Buffer | Self::Delimiter)
    }
}

impl Component {
    pub fn short(&self) -> &'static str {
        match self {
            Self::Beginning => "b",
            Self::Contents => "c",
            Self::End => "e",
            Self::Value => "v",
            Self::Parameters => "p",
            Self::Arguments => "a",
            Self::Name => "n",
            Self::Self_ => "s",
            Self::Word => "w",
            Self::Definition => "d",
        }
    }

    pub fn from_short(s: &str) -> Option<Self> {
        match s {
            "b" => Some(Self::Beginning),
            "c" => Some(Self::Contents),
            "e" => Some(Self::End),
            "v" => Some(Self::Value),
            "p" => Some(Self::Parameters),
            "a" => Some(Self::Arguments),
            "n" => Some(Self::Name),
            "s" => Some(Self::Self_),
            "w" => Some(Self::Word),
            "d" => Some(Self::Definition),
            _ => None,
        }
    }
}

pub fn is_valid_combination(scope: Scope, component: Component) -> bool {
    match (scope, component) {
        (Scope::Line | Scope::Buffer, Component::Word) => true,
        (_, Component::Word) => false,
        (Scope::Function | Scope::Variable | Scope::Struct, Component::Definition) => true,
        (_, Component::Definition) => false,
        (Scope::Delimiter, Component::Beginning) => true,
        (Scope::Delimiter, Component::Contents) => true,
        (Scope::Delimiter, Component::End) => true,
        (Scope::Delimiter, Component::Self_) => true,
        (Scope::Delimiter, Component::Name) => true,
        (Scope::Delimiter, Component::Value) => false,
        (Scope::Delimiter, Component::Parameters) => false,
        (Scope::Delimiter, Component::Arguments) => false,
        (_, Component::End | Component::Name | Component::Self_) => true,
        (Scope::Line | Scope::Buffer, Component::Beginning) => true,
        (_, Component::Beginning) => false,
        (Scope::Function | Scope::Struct, Component::Contents) => true,
        (_, Component::Contents) => false,
        (Scope::Line | Scope::Buffer, Component::Value) => false,
        (Scope::Function | Scope::Struct, Component::Value) => false,
        (Scope::Variable, Component::Value) => true,
        (Scope::Member, Component::Value) => true,
        (Scope::Function, Component::Parameters) => true,
        (Scope::Function, Component::Arguments) => true,
        (_, Component::Parameters | Component::Arguments) => false,
    }
}

pub fn is_valid_list_positional(positional: Positional) -> bool {
    !matches!(positional, Positional::Outside)
}

pub fn is_valid_jump_combination(positional: Positional, component: Component) -> bool {
    match positional {
        Positional::Outside => matches!(component, Component::Beginning | Component::End),
        _ => !matches!(
            component,
            Component::Value | Component::Parameters | Component::Arguments
        ),
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Change => write!(f, "Change"),
            Self::Replace => write!(f, "Replace"),
            Self::Delete => write!(f, "Delete"),
            Self::Yank => write!(f, "Yank"),
            Self::Append => write!(f, "Append"),
            Self::Prepend => write!(f, "Prepend"),
            Self::Insert => write!(f, "Insert"),
            Self::Jump => write!(f, "Jump"),
            Self::List => write!(f, "List"),
        }
    }
}

impl fmt::Display for Positional {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inside => write!(f, "Inside"),
            Self::Until => write!(f, "Until"),
            Self::After => write!(f, "After"),
            Self::Before => write!(f, "Before"),
            Self::Next => write!(f, "Next"),
            Self::Previous => write!(f, "Previous"),
            Self::Entire => write!(f, "Entire"),
            Self::Outside => write!(f, "Outside"),
            Self::To => write!(f, "To"),
            Self::Last => write!(f, "Last"),
            Self::First => write!(f, "First"),
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Line => write!(f, "Line"),
            Self::Buffer => write!(f, "Buffer"),
            Self::Function => write!(f, "Function"),
            Self::Variable => write!(f, "Variable"),
            Self::Struct => write!(f, "Struct"),
            Self::Member => write!(f, "Member"),
            Self::Delimiter => write!(f, "Delimiter"),
        }
    }
}

impl fmt::Display for Component {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Beginning => write!(f, "Beginning"),
            Self::Contents => write!(f, "Contents"),
            Self::End => write!(f, "End"),
            Self::Value => write!(f, "Value"),
            Self::Parameters => write!(f, "Parameters"),
            Self::Arguments => write!(f, "Arguments"),
            Self::Name => write!(f, "Name"),
            Self::Self_ => write!(f, "Self"),
            Self::Word => write!(f, "Word"),
            Self::Definition => write!(f, "Definition"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_short_round_trip() {
        for action in [
            Action::Change,
            Action::Replace,
            Action::Delete,
            Action::Yank,
            Action::Append,
            Action::Prepend,
            Action::Insert,
            Action::Jump,
            Action::List,
        ] {
            let short = action.short();
            assert_eq!(Action::from_short(short), Some(action));
        }
    }

    #[test]
    fn positional_short_round_trip() {
        for positional in [
            Positional::Inside,
            Positional::Until,
            Positional::After,
            Positional::Before,
            Positional::Next,
            Positional::Previous,
            Positional::Entire,
            Positional::Outside,
            Positional::To,
            Positional::Last,
            Positional::First,
        ] {
            let short = positional.short();
            assert_eq!(Positional::from_short(short), Some(positional));
        }
    }

    #[test]
    fn scope_short_round_trip() {
        for scope in [
            Scope::Line,
            Scope::Buffer,
            Scope::Function,
            Scope::Variable,
            Scope::Struct,
            Scope::Member,
            Scope::Delimiter,
        ] {
            let short = scope.short();
            assert_eq!(Scope::from_short(short), Some(scope));
        }
    }

    #[test]
    fn component_short_round_trip() {
        for component in [
            Component::Beginning,
            Component::Contents,
            Component::End,
            Component::Value,
            Component::Parameters,
            Component::Arguments,
            Component::Name,
            Component::Self_,
            Component::Word,
            Component::Definition,
        ] {
            let short = component.short();
            assert_eq!(Component::from_short(short), Some(component));
        }
    }

    #[test]
    fn scope_lsp_requirement() {
        assert!(!Scope::Line.requires_lsp());
        assert!(!Scope::Buffer.requires_lsp());
        assert!(Scope::Function.requires_lsp());
        assert!(Scope::Variable.requires_lsp());
        assert!(Scope::Struct.requires_lsp());
        assert!(Scope::Member.requires_lsp());
    }

    #[test]
    fn valid_combinations() {
        assert!(is_valid_combination(Scope::Function, Component::Parameters));
        assert!(is_valid_combination(Scope::Variable, Component::Value));
        assert!(is_valid_combination(Scope::Member, Component::Value));
        assert!(is_valid_combination(Scope::Function, Component::Contents));
        assert!(is_valid_combination(Scope::Struct, Component::Contents));
        assert!(is_valid_combination(Scope::Line, Component::Beginning));
        assert!(is_valid_combination(Scope::Buffer, Component::Beginning));
        assert!(is_valid_combination(Scope::Line, Component::End));
        assert!(is_valid_combination(Scope::Line, Component::Self_));
    }

    #[test]
    fn invalid_combinations() {
        assert!(!is_valid_combination(Scope::Line, Component::Parameters));
        assert!(!is_valid_combination(Scope::Line, Component::Value));
        assert!(!is_valid_combination(Scope::Line, Component::Contents));
        assert!(!is_valid_combination(Scope::Buffer, Component::Contents));
        assert!(!is_valid_combination(Scope::Buffer, Component::Parameters));
        assert!(!is_valid_combination(Scope::Variable, Component::Contents));
        assert!(!is_valid_combination(Scope::Function, Component::Beginning));
        assert!(!is_valid_combination(Scope::Function, Component::Value));
        assert!(!is_valid_combination(Scope::Struct, Component::Value));
        assert!(!is_valid_combination(
            Scope::Variable,
            Component::Parameters
        ));
        assert!(!is_valid_combination(Scope::Struct, Component::Parameters));
    }

    #[test]
    fn each_position_has_unique_short_letters() {
        let actions = ["c", "r", "d", "y", "a", "p", "i", "j", "l"];
        let positionals = ["i", "u", "a", "b", "n", "p", "e", "o", "t", "l", "f"];
        let scopes = ["l", "b", "f", "v", "s", "m", "d"];
        let components = ["b", "c", "e", "v", "p", "a", "n", "s", "w", "d"];
        for set in [&actions[..], &positionals[..], &scopes[..], &components[..]] {
            let mut seen = std::collections::HashSet::new();
            for s in set {
                assert!(seen.insert(*s), "duplicate short letter '{s}' in {:?}", set);
            }
        }
    }

    #[test]
    fn all_combinations_are_either_valid_or_invalid_no_panics() {
        let scopes = [
            Scope::Line,
            Scope::Buffer,
            Scope::Function,
            Scope::Variable,
            Scope::Struct,
            Scope::Member,
            Scope::Delimiter,
        ];
        let components = [
            Component::Beginning,
            Component::Contents,
            Component::End,
            Component::Value,
            Component::Parameters,
            Component::Arguments,
            Component::Name,
            Component::Self_,
            Component::Word,
            Component::Definition,
        ];
        for s in &scopes {
            for c in &components {
                let _ = is_valid_combination(*s, *c);
            }
        }
    }

    #[test]
    fn display_formats() {
        assert_eq!(format!("{}", Action::Change), "Change");
        assert_eq!(format!("{}", Positional::Inside), "Inside");
        assert_eq!(format!("{}", Scope::Function), "Function");
        assert_eq!(format!("{}", Component::Self_), "Self");
    }

    // --- work item 0005: Jump / To / Delimiter ---

    #[test]
    fn action_jump_requires_interactive_only() {
        assert!(Action::Jump.requires_interactive());
        assert!(!Action::Change.requires_interactive());
        assert!(!Action::Delete.requires_interactive());
        assert!(!Action::Replace.requires_interactive());
        assert!(!Action::Yank.requires_interactive());
        assert!(!Action::Append.requires_interactive());
        assert!(!Action::Prepend.requires_interactive());
        assert!(!Action::Insert.requires_interactive());
    }

    #[test]
    fn scope_delimiter_does_not_require_lsp() {
        assert!(!Scope::Delimiter.requires_lsp());
    }

    #[test]
    fn is_valid_jump_combination_outside_beginning_and_end_pass() {
        assert!(is_valid_jump_combination(
            Positional::Outside,
            Component::Beginning
        ));
        assert!(is_valid_jump_combination(
            Positional::Outside,
            Component::End
        ));
    }

    #[test]
    fn is_valid_jump_combination_outside_other_components_fail() {
        assert!(!is_valid_jump_combination(
            Positional::Outside,
            Component::Value
        ));
        assert!(!is_valid_jump_combination(
            Positional::Outside,
            Component::Contents
        ));
        assert!(!is_valid_jump_combination(
            Positional::Outside,
            Component::Arguments
        ));
        assert!(!is_valid_jump_combination(
            Positional::Outside,
            Component::Parameters
        ));
        assert!(!is_valid_jump_combination(
            Positional::Outside,
            Component::Name
        ));
        assert!(!is_valid_jump_combination(
            Positional::Outside,
            Component::Self_
        ));
    }

    #[test]
    fn is_valid_jump_combination_non_outside_valid_combinations() {
        assert!(is_valid_jump_combination(
            Positional::To,
            Component::Contents
        ));
        assert!(is_valid_jump_combination(Positional::Next, Component::Name));
        assert!(is_valid_jump_combination(
            Positional::Inside,
            Component::Contents
        ));
    }

    #[test]
    fn is_valid_jump_combination_value_params_args_always_fail() {
        for pos in [
            Positional::Inside,
            Positional::Until,
            Positional::After,
            Positional::Before,
            Positional::Next,
            Positional::Previous,
            Positional::Entire,
            Positional::To,
            Positional::Last,
            Positional::First,
        ] {
            assert!(
                !is_valid_jump_combination(pos, Component::Value),
                "Value should fail for {pos:?}"
            );
            assert!(
                !is_valid_jump_combination(pos, Component::Parameters),
                "Parameters should fail for {pos:?}"
            );
            assert!(
                !is_valid_jump_combination(pos, Component::Arguments),
                "Arguments should fail for {pos:?}"
            );
        }
    }

    #[test]
    fn is_valid_combination_delimiter_allowed_components() {
        assert!(is_valid_combination(Scope::Delimiter, Component::Beginning));
        assert!(is_valid_combination(Scope::Delimiter, Component::Contents));
        assert!(is_valid_combination(Scope::Delimiter, Component::End));
        assert!(is_valid_combination(Scope::Delimiter, Component::Self_));
        assert!(is_valid_combination(Scope::Delimiter, Component::Name));
    }

    #[test]
    fn is_valid_combination_delimiter_forbidden_components() {
        assert!(!is_valid_combination(Scope::Delimiter, Component::Value));
        assert!(!is_valid_combination(
            Scope::Delimiter,
            Component::Parameters
        ));
        assert!(!is_valid_combination(
            Scope::Delimiter,
            Component::Arguments
        ));
    }

    // --- work item 0011: Word / Definition / List ---

    #[test]
    fn word_component_valid_combinations() {
        assert!(is_valid_combination(Scope::Line, Component::Word));
        assert!(is_valid_combination(Scope::Buffer, Component::Word));
    }

    #[test]
    fn word_component_invalid_combinations() {
        assert!(!is_valid_combination(Scope::Function, Component::Word));
        assert!(!is_valid_combination(Scope::Struct, Component::Word));
        assert!(!is_valid_combination(Scope::Variable, Component::Word));
        assert!(!is_valid_combination(Scope::Member, Component::Word));
        assert!(!is_valid_combination(Scope::Delimiter, Component::Word));
    }

    #[test]
    fn definition_component_valid_combinations() {
        assert!(is_valid_combination(Scope::Function, Component::Definition));
        assert!(is_valid_combination(Scope::Variable, Component::Definition));
        assert!(is_valid_combination(Scope::Struct, Component::Definition));
    }

    #[test]
    fn definition_component_invalid_combinations() {
        assert!(!is_valid_combination(Scope::Line, Component::Definition));
        assert!(!is_valid_combination(Scope::Buffer, Component::Definition));
        assert!(!is_valid_combination(Scope::Delimiter, Component::Definition));
        assert!(!is_valid_combination(Scope::Member, Component::Definition));
    }

    #[test]
    fn is_valid_list_positional_outside_false() {
        assert!(!is_valid_list_positional(Positional::Outside));
    }

    #[test]
    fn is_valid_list_positional_all_others_true() {
        for pos in [
            Positional::Inside,
            Positional::Until,
            Positional::After,
            Positional::Before,
            Positional::Next,
            Positional::Previous,
            Positional::Entire,
            Positional::To,
            Positional::Last,
            Positional::First,
        ] {
            assert!(
                is_valid_list_positional(pos),
                "{pos:?} should be valid for List"
            );
        }
    }

    #[test]
    fn action_list_requires_not_interactive() {
        assert!(!Action::List.requires_interactive());
    }
}
