use anyhow::Result;

use crate::data::chord_types::{
    Action, Component, Positional, Scope, is_valid_combination, is_valid_jump_combination,
};

use super::errors::ChordError;
use super::types::{ChordArgs, ChordQuery};

pub fn parse(input: &str) -> Result<ChordQuery> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ChordError::parse(input, 0, "empty chord input").into());
    }

    let (chord_part, raw_args) = split_chord_and_args(input)?;

    if let Some(query) = try_parse_short_form(chord_part, &raw_args, input)? {
        return Ok(query);
    }

    if let Some(query) = try_parse_long_form(chord_part, &raw_args, input)? {
        return Ok(query);
    }

    let suggestion = suggest_chord(chord_part);
    match suggestion {
        Some(sug) => Err(ChordError::parse_with_suggestion(
            input,
            0,
            format!("unknown chord '{chord_part}'"),
            sug,
        )
        .into()),
        None => Err(ChordError::parse(input, 0, format!("unknown chord '{chord_part}'")).into()),
    }
}

fn split_chord_and_args(input: &str) -> Result<(&str, Option<&str>)> {
    let Some(paren_start) = input.find('(') else {
        return Ok((input, None));
    };
    if !input.ends_with(')') {
        return Err(ChordError::parse(
            input,
            paren_start,
            "unterminated argument list (missing closing ')')",
        )
        .into());
    }
    let chord_part = &input[..paren_start];
    let args_content = &input[paren_start + 1..input.len() - 1];
    Ok((chord_part, Some(args_content)))
}

fn parse_args(raw_args: &Option<&str>) -> ChordArgs {
    let mut args = ChordArgs::default();
    let raw = match raw_args {
        Some(s) if !s.is_empty() => *s,
        _ => return args,
    };

    for pair in split_kv_pairs(raw) {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((key, val)) = pair.split_once(':') {
            let key = key.trim();
            let val = val.trim().trim_matches('"');
            match key {
                "target" if !val.is_empty() => {
                    args.target_name = Some(val.to_string());
                    args.target_line = val.parse().ok();
                }
                "parent" => {
                    args.parent_name = Some(val.to_string());
                }
                "cursor" => {
                    if let Some((l, c)) = val.split_once(',') {
                        if let (Ok(line), Ok(col)) = (l.trim().parse(), c.trim().parse()) {
                            args.cursor_pos = Some((line, col));
                        }
                    }
                }
                "value" => {
                    args.value = Some(val.to_string());
                }
                "find" => {
                    args.find = Some(val.to_string());
                }
                "replace" => {
                    args.replace = Some(val.to_string());
                }
                _ => {}
            }
        }
    }

    args
}

fn split_kv_pairs(input: &str) -> Vec<&str> {
    let mut pairs = Vec::new();
    let mut depth = 0;
    let mut in_quotes = false;
    let mut start = 0;

    for (i, ch) in input.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => {
                depth += 1;
            }
            ')' if !in_quotes => {
                depth -= 1;
            }
            ',' if !in_quotes && depth == 0 => {
                pairs.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < input.len() {
        pairs.push(&input[start..]);
    }
    pairs
}

fn try_parse_short_form(
    chord_part: &str,
    raw_args: &Option<&str>,
    _original_input: &str,
) -> Result<Option<ChordQuery>> {
    if chord_part.len() != 4 {
        return Ok(None);
    }

    let chars: Vec<&str> = chord_part
        .char_indices()
        .map(|(i, c)| &chord_part[i..i + c.len_utf8()])
        .collect();

    if chars.len() != 4 {
        return Ok(None);
    }

    let action = match Action::from_short(chars[0]) {
        Some(a) => a,
        None => return Ok(None),
    };
    let positional = match Positional::from_short(chars[1]) {
        Some(p) => p,
        None => return Ok(None),
    };
    let scope = match Scope::from_short(chars[2]) {
        Some(s) => s,
        None => return Ok(None),
    };
    let component = match Component::from_short(chars[3]) {
        Some(c) => c,
        None => return Ok(None),
    };

    if !is_valid_combination(scope, component) {
        return Err(ChordError::invalid_combination(scope, component).into());
    }

    if matches!(scope, Scope::Delimiter)
        && matches!(positional, Positional::Next | Positional::Previous)
    {
        return Err(ChordError::parse(
            _original_input,
            0,
            "Next/Previous positional is not valid for Delimiter scope",
        )
        .into());
    }

    if action == Action::Jump && !is_valid_jump_combination(positional, component) {
        let msg = if positional == Positional::Outside {
            "Jump with Outside positional requires Beginning or End component to specify direction"
        } else {
            "Jump does not operate on Value, Parameters, or Arguments components"
        };
        return Err(ChordError::parse(_original_input, 0, msg).into());
    }

    let args = parse_args(raw_args);

    if action == Action::Jump && args.value.is_some() {
        return Err(
            ChordError::parse(_original_input, 0, "Jump does not accept a value argument").into(),
        );
    }

    Ok(Some(ChordQuery {
        action,
        positional,
        scope,
        component,
        args,
        requires_lsp: scope.requires_lsp(),
    }))
}

fn try_parse_long_form(
    chord_part: &str,
    raw_args: &Option<&str>,
    _original_input: &str,
) -> Result<Option<ChordQuery>> {
    let (action, rest) = match parse_long_action(chord_part) {
        Some(r) => r,
        None => return Ok(None),
    };
    let (positional, rest) = match parse_long_positional(rest) {
        Some(r) => r,
        None => return Ok(None),
    };
    let (scope, rest) = match parse_long_scope(rest) {
        Some(r) => r,
        None => return Ok(None),
    };
    let component = match parse_long_component(rest) {
        Some(c) => c,
        None => return Ok(None),
    };

    if !is_valid_combination(scope, component) {
        return Err(ChordError::invalid_combination(scope, component).into());
    }

    if matches!(scope, Scope::Delimiter)
        && matches!(positional, Positional::Next | Positional::Previous)
    {
        return Err(ChordError::parse(
            _original_input,
            0,
            "Next/Previous positional is not valid for Delimiter scope",
        )
        .into());
    }

    if action == Action::Jump && !is_valid_jump_combination(positional, component) {
        let msg = if positional == Positional::Outside {
            "Jump with Outside positional requires Beginning or End component to specify direction"
        } else {
            "Jump does not operate on Value, Parameters, or Arguments components"
        };
        return Err(ChordError::parse(_original_input, 0, msg).into());
    }

    let args = parse_args(raw_args);

    if action == Action::Jump && args.value.is_some() {
        return Err(
            ChordError::parse(_original_input, 0, "Jump does not accept a value argument").into(),
        );
    }

    Ok(Some(ChordQuery {
        action,
        positional,
        scope,
        component,
        args,
        requires_lsp: scope.requires_lsp(),
    }))
}

fn parse_long_action(input: &str) -> Option<(Action, &str)> {
    let pairs = [
        ("Change", Action::Change),
        ("Replace", Action::Replace),
        ("Delete", Action::Delete),
        ("Yank", Action::Yank),
        ("Append", Action::Append),
        ("Prepend", Action::Prepend),
        ("Insert", Action::Insert),
        ("Jump", Action::Jump),
    ];
    for (prefix, action) in pairs {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Some((action, rest));
        }
    }
    None
}

fn parse_long_positional(input: &str) -> Option<(Positional, &str)> {
    let pairs = [
        ("Inside", Positional::Inside),
        ("Until", Positional::Until),
        ("After", Positional::After),
        ("Before", Positional::Before),
        ("Next", Positional::Next),
        ("Previous", Positional::Previous),
        ("Entire", Positional::Entire),
        ("Outside", Positional::Outside),
        ("To", Positional::To),
    ];
    for (prefix, positional) in pairs {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Some((positional, rest));
        }
    }
    None
}

fn parse_long_scope(input: &str) -> Option<(Scope, &str)> {
    let pairs = [
        ("Function", Scope::Function),
        ("Variable", Scope::Variable),
        ("Delimiter", Scope::Delimiter),
        ("Buffer", Scope::Buffer),
        ("Struct", Scope::Struct),
        ("Member", Scope::Member),
        ("Line", Scope::Line),
    ];
    for (prefix, scope) in pairs {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Some((scope, rest));
        }
    }
    None
}

fn parse_long_component(input: &str) -> Option<Component> {
    match input {
        "Beginning" => Some(Component::Beginning),
        "Contents" => Some(Component::Contents),
        "End" => Some(Component::End),
        "Value" => Some(Component::Value),
        "Parameters" => Some(Component::Parameters),
        "Arguments" => Some(Component::Arguments),
        "Name" => Some(Component::Name),
        "Self" => Some(Component::Self_),
        _ => None,
    }
}

fn suggest_chord(input: &str) -> Option<String> {
    let input_chars: String = input.chars().take(4).collect();
    if input_chars.chars().count() < 4 {
        return None;
    }

    let actions = ['c', 'r', 'd', 'y', 'a', 'p', 'i', 'j'];
    let positionals = ['i', 'u', 'a', 'b', 'n', 'p', 'e', 'o', 't'];
    let scopes = ['l', 'b', 'f', 'v', 's', 'm', 'd'];
    let components = ['b', 'c', 'e', 'v', 'p', 'a', 'n', 's'];

    let mut best_dist = usize::MAX;
    let mut best = None;

    for &a in &actions {
        for &p in &positionals {
            for &s in &scopes {
                for &c in &components {
                    let candidate = format!("{a}{p}{s}{c}");
                    let scope = Scope::from_short(&s.to_string()).unwrap();
                    let comp = Component::from_short(&c.to_string()).unwrap();
                    if !is_valid_combination(scope, comp) {
                        continue;
                    }
                    let dist = levenshtein(&input_chars, &candidate);
                    if dist < best_dist && dist <= 2 {
                        best_dist = dist;
                        best = Some(candidate);
                    }
                }
            }
        }
    }

    best
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];

    for (i, row) in dp.iter_mut().enumerate().take(a.len() + 1) {
        row[0] = i;
    }
    for (j, val) in dp[0].iter_mut().enumerate().take(b.len() + 1) {
        *val = j;
    }

    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::data::chord_types::{
        Action, Component, Positional, Scope, is_valid_combination, is_valid_jump_combination,
    };

    const ALL_ACTIONS: &[Action] = &[
        Action::Change,
        Action::Replace,
        Action::Delete,
        Action::Yank,
        Action::Append,
        Action::Prepend,
        Action::Insert,
        Action::Jump,
    ];

    const ALL_POSITIONALS: &[Positional] = &[
        Positional::Inside,
        Positional::Until,
        Positional::After,
        Positional::Before,
        Positional::Next,
        Positional::Previous,
        Positional::Entire,
        Positional::Outside,
        Positional::To,
    ];

    const ALL_SCOPES: &[Scope] = &[
        Scope::Line,
        Scope::Buffer,
        Scope::Function,
        Scope::Variable,
        Scope::Struct,
        Scope::Member,
        Scope::Delimiter,
    ];

    const ALL_COMPONENTS: &[Component] = &[
        Component::Beginning,
        Component::Contents,
        Component::End,
        Component::Value,
        Component::Parameters,
        Component::Arguments,
        Component::Name,
        Component::Self_,
    ];

    #[test]
    fn all_valid_short_forms_parse_and_invalid_fail() {
        for &action in ALL_ACTIONS {
            for &pos in ALL_POSITIONALS {
                for &scope in ALL_SCOPES {
                    for &comp in ALL_COMPONENTS {
                        let short = format!(
                            "{}{}{}{}",
                            action.short(),
                            pos.short(),
                            scope.short(),
                            comp.short()
                        );
                        let result = parse(&short);
                        let scope_comp_valid = is_valid_combination(scope, comp);
                        let jump_valid =
                            action != Action::Jump || is_valid_jump_combination(pos, comp);
                        let delimiter_positional_valid = scope != Scope::Delimiter
                            || !matches!(pos, Positional::Next | Positional::Previous);
                        let should_parse =
                            scope_comp_valid && jump_valid && delimiter_positional_valid;
                        if should_parse {
                            let q = result.unwrap_or_else(|e| {
                                panic!("expected {short} to parse OK, got: {e}")
                            });
                            assert_eq!(q.action, action, "action mismatch for {short}");
                            assert_eq!(q.positional, pos, "positional mismatch for {short}");
                            assert_eq!(q.scope, scope, "scope mismatch for {short}");
                            assert_eq!(q.component, comp, "component mismatch for {short}");
                            assert_eq!(
                                q.requires_lsp,
                                scope.requires_lsp(),
                                "requires_lsp mismatch for {short}"
                            );
                        } else {
                            assert!(
                                result.is_err(),
                                "expected {short} to fail (invalid combo), but it parsed OK"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn all_valid_long_forms_parse() {
        for &action in ALL_ACTIONS {
            for &pos in ALL_POSITIONALS {
                for &scope in ALL_SCOPES {
                    for &comp in ALL_COMPONENTS {
                        if !is_valid_combination(scope, comp) {
                            continue;
                        }
                        if action == Action::Jump && !is_valid_jump_combination(pos, comp) {
                            continue;
                        }
                        if scope == Scope::Delimiter
                            && matches!(pos, Positional::Next | Positional::Previous)
                        {
                            continue;
                        }
                        let long = format!("{action}{pos}{scope}{comp}");
                        let result = parse(&long);
                        let q = result
                            .unwrap_or_else(|e| panic!("expected {long} to parse OK, got: {e}"));
                        assert_eq!(q.action, action, "action mismatch for {long}");
                        assert_eq!(q.positional, pos, "positional mismatch for {long}");
                        assert_eq!(q.scope, scope, "scope mismatch for {long}");
                        assert_eq!(q.component, comp, "component mismatch for {long}");
                        assert_eq!(q.requires_lsp, scope.requires_lsp());
                    }
                }
            }
        }
    }

    #[test]
    fn spot_check_change_inside_function_contents() {
        let q = parse("cifc").unwrap();
        assert_eq!(q.action, Action::Change);
        assert_eq!(q.positional, Positional::Inside);
        assert_eq!(q.scope, Scope::Function);
        assert_eq!(q.component, Component::Contents);
        assert!(q.requires_lsp);
    }

    #[test]
    fn spot_check_delete_entire_line_self() {
        let q = parse("dels").unwrap();
        assert_eq!(q.action, Action::Delete);
        assert_eq!(q.positional, Positional::Entire);
        assert_eq!(q.scope, Scope::Line);
        assert_eq!(q.component, Component::Self_);
        assert!(!q.requires_lsp);
    }

    #[test]
    fn spot_check_yank_entire_struct_self() {
        let q = parse("yess").unwrap();
        assert_eq!(q.action, Action::Yank);
        assert_eq!(q.positional, Positional::Entire);
        assert_eq!(q.scope, Scope::Struct);
        assert_eq!(q.component, Component::Self_);
        assert!(q.requires_lsp);
    }

    #[test]
    fn spot_check_append_after_line_end() {
        let q = parse("aale").unwrap();
        assert_eq!(q.action, Action::Append);
        assert_eq!(q.positional, Positional::After);
        assert_eq!(q.scope, Scope::Line);
        assert_eq!(q.component, Component::End);
        assert!(!q.requires_lsp);
    }

    #[test]
    fn spot_check_buffer_contents_is_invalid() {
        assert!(parse("pbbc").is_err());
    }

    #[test]
    fn spot_check_change_inside_function_beginning_is_invalid() {
        assert!(parse("cifb").is_err());
    }

    #[test]
    fn spot_check_replace_entire_variable_name() {
        let q = parse("revn").unwrap();
        assert_eq!(q.action, Action::Replace);
        assert_eq!(q.positional, Positional::Entire);
        assert_eq!(q.scope, Scope::Variable);
        assert_eq!(q.component, Component::Name);
    }

    #[test]
    fn spot_check_insert_until_member_value() {
        let q = parse("iumv").unwrap();
        assert_eq!(q.action, Action::Insert);
        assert_eq!(q.positional, Positional::Until);
        assert_eq!(q.scope, Scope::Member);
        assert_eq!(q.component, Component::Value);
    }

    #[test]
    fn short_form_and_long_form_equivalent() {
        let short = parse("cifc").unwrap();
        let long = parse("ChangeInsideFunctionContents").unwrap();
        assert_eq!(short.action, long.action);
        assert_eq!(short.positional, long.positional);
        assert_eq!(short.scope, long.scope);
        assert_eq!(short.component, long.component);
    }

    #[test]
    fn args_target_key() {
        let q = parse("cifc(target:getData)").unwrap();
        assert_eq!(q.args.target_name.as_deref(), Some("getData"));
        assert!(q.args.target_line.is_none());
        assert!(q.args.cursor_pos.is_none());
        assert!(q.args.value.is_none());
    }

    #[test]
    fn args_target_key_works_for_all_lsp_scopes() {
        let q = parse("cevv(target:myVar)").unwrap();
        assert_eq!(q.args.target_name.as_deref(), Some("myVar"));
        let q = parse("cesn(target:MyStruct)").unwrap();
        assert_eq!(q.args.target_name.as_deref(), Some("MyStruct"));
        let q = parse("cemn(target:myField)").unwrap();
        assert_eq!(q.args.target_name.as_deref(), Some("myField"));
    }

    #[test]
    fn args_old_scope_specific_keys_are_ignored() {
        let q = parse("cifc(function:getData)").unwrap();
        assert!(q.args.target_name.is_none());
        let q = parse("cevv(variable:myVar)").unwrap();
        assert!(q.args.target_name.is_none());
        let q = parse("cesn(struct:MyStruct)").unwrap();
        assert!(q.args.target_name.is_none());
        let q = parse("cemn(member:myField)").unwrap();
        assert!(q.args.target_name.is_none());
        let q = parse("cifc(name:myFunc)").unwrap();
        assert!(q.args.target_name.is_none());
        let q = parse("cels(line:42)").unwrap();
        assert!(q.args.target_line.is_none());
    }

    #[test]
    fn args_target_line_number() {
        let q = parse("cels(target:42)").unwrap();
        assert_eq!(q.args.target_line, Some(42));
        assert_eq!(q.args.target_name.as_deref(), Some("42"));
    }

    #[test]
    fn args_cursor_position() {
        let q = parse(r#"cels(cursor:"3,7")"#).unwrap();
        assert_eq!(q.args.cursor_pos, Some((3, 7)));
    }

    #[test]
    fn args_cursor_position_with_spaces() {
        let q = parse(r#"cels(cursor:"0,12")"#).unwrap();
        assert_eq!(q.args.cursor_pos, Some((0, 12)));
    }

    #[test]
    fn args_value_plain() {
        let q = parse("cels(value:hello)").unwrap();
        assert_eq!(q.args.value.as_deref(), Some("hello"));
    }

    #[test]
    fn args_value_quoted_with_spaces() {
        let q = parse(r#"cifc(target:getData, value:"new body goes here")"#).unwrap();
        assert_eq!(q.args.target_name.as_deref(), Some("getData"));
        assert_eq!(q.args.value.as_deref(), Some("new body goes here"));
    }

    #[test]
    fn args_value_with_parens_quoted() {
        let q = parse(r#"cifp(target:getData, value:"(x: i32)")"#).unwrap();
        assert_eq!(q.args.value.as_deref(), Some("(x: i32)"));
    }

    #[test]
    fn args_extra_commas_ignored() {
        let q = parse("cels(,target:1,,)").unwrap();
        assert_eq!(q.args.target_line, Some(1));
    }

    #[test]
    fn args_missing_value_for_target_is_none() {
        let q = parse("cels(target:)").unwrap();
        assert!(q.args.target_line.is_none());
        assert!(q.args.target_name.is_none());
    }

    #[test]
    fn args_unknown_key_is_ignored() {
        let q = parse("cels(bogus:foo, target:2)").unwrap();
        assert_eq!(q.args.target_line, Some(2));
    }

    #[test]
    fn args_multiple_keys() {
        let q = parse(r#"cifc(target:getData, value:"body")"#).unwrap();
        assert_eq!(q.args.target_name.as_deref(), Some("getData"));
        assert_eq!(q.args.value.as_deref(), Some("body"));
    }

    #[test]
    fn invalid_combination_line_parameters_short() {
        let result = parse("cilp");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.to_lowercase().contains("invalid"),
            "expected 'invalid' in error: {msg}"
        );
    }

    #[test]
    fn invalid_combination_buffer_value_short() {
        let result = parse("cibv");
        assert!(result.is_err());
    }

    #[test]
    fn invalid_combination_variable_parameters() {
        let result = parse("civp");
        assert!(result.is_err());
    }

    #[test]
    fn invalid_combination_struct_arguments() {
        let result = parse("cisa");
        assert!(result.is_err());
    }

    #[test]
    fn invalid_combination_long_form() {
        let result = parse("ChangeInsideLineParameters");
        assert!(result.is_err());
    }

    #[test]
    fn invalid_combination_long_form_buffer_value() {
        let result = parse("ChangeInsideBufferValue");
        assert!(result.is_err());
    }

    #[test]
    fn empty_input_errors() {
        let result = parse("");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("empty"));
    }

    #[test]
    fn whitespace_only_errors() {
        let result = parse("   ");
        assert!(result.is_err());
    }

    #[test]
    fn unknown_short_chord_errors() {
        let result = parse("zzzz");
        assert!(result.is_err());
    }

    #[test]
    fn near_miss_suggests_correction() {
        let result = parse("xifv");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("did you mean"),
            "expected suggestion in error message: {msg}"
        );
    }

    #[test]
    fn whitespace_trimmed_around_chord() {
        let q = parse("  cifc  ").unwrap();
        assert_eq!(q.action, Action::Change);
        assert_eq!(q.positional, Positional::Inside);
        assert_eq!(q.scope, Scope::Function);
        assert_eq!(q.component, Component::Contents);
    }

    #[test]
    fn short_form_sets_requires_lsp_false_for_line_and_buffer() {
        assert!(!parse("cels").unwrap().requires_lsp);
        assert!(!parse("cebs").unwrap().requires_lsp);
    }

    #[test]
    fn short_form_sets_requires_lsp_true_for_lsp_scopes() {
        assert!(parse("cefs").unwrap().requires_lsp);
        assert!(parse("cevs").unwrap().requires_lsp);
        assert!(parse("cess").unwrap().requires_lsp);
        assert!(parse("cems").unwrap().requires_lsp);
    }

    #[test]
    fn long_form_self_component_accepted() {
        let q = parse("ChangeEntireLineSelf").unwrap();
        assert_eq!(q.component, Component::Self_);
    }

    #[test]
    fn unterminated_paren_errors() {
        let result = parse("cifv(target:1");
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("unterminated"));
    }

    #[test]
    fn args_parent_key() {
        let q = parse("cemv(target:x, parent:Foo)").unwrap();
        assert_eq!(q.args.target_name.as_deref(), Some("x"));
        assert_eq!(q.args.parent_name.as_deref(), Some("Foo"));
    }

    #[test]
    fn args_find_replace_keys() {
        let q = parse(r#"rels(target:0, find:"foo", replace:"bar")"#).unwrap();
        assert_eq!(q.args.find.as_deref(), Some("foo"));
        assert_eq!(q.args.replace.as_deref(), Some("bar"));
    }

    #[test]
    fn unicode_input_does_not_panic_in_suggest() {
        let result = parse("cłfv");
        assert!(result.is_err());
    }

    // --- work item 0005: Jump / To / Delimiter ---

    #[test]
    fn jump_outside_invalid_component_rejects_with_direction_hint() {
        // joln = Jump Outside Line Name — valid scope/component, invalid jump+outside
        let result = parse("joln");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("Beginning") || msg.contains("End") || msg.contains("direction"),
            "expected direction hint in error: {msg}"
        );
    }

    #[test]
    fn jump_outside_beginning_and_end_are_valid() {
        // jolb = Jump Outside Line Beginning; jole = Jump Outside Line End
        assert!(parse("jolb").is_ok(), "jolb should parse OK");
        assert!(parse("jole").is_ok(), "jole should parse OK");
    }

    #[test]
    fn jump_outside_other_components_fail() {
        // Name and Self_ are valid scope/component combos but invalid for Jump+Outside
        assert!(parse("joln").is_err(), "joln (Name) should fail");
        assert!(parse("jols").is_err(), "jols (Self_) should fail");
        // Parameters is a valid Function component but invalid for Jump+Outside
        assert!(parse("jofp").is_err(), "jofp (Parameters) should fail");
    }

    #[test]
    fn jump_non_outside_valid_combinations() {
        assert!(
            parse("jtfc").is_ok(),
            "jtfc (To Function Contents) should parse OK"
        );
        assert!(
            parse("jnfn").is_ok(),
            "jnfn (Next Function Name) should parse OK"
        );
        assert!(
            parse("jifc").is_ok(),
            "jifc (Inside Function Contents) should parse OK"
        );
    }

    #[test]
    fn jump_with_value_argument_rejects() {
        let result = parse(r#"jtfc(value:"text")"#);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("value") || msg.contains("Jump"),
            "expected value/Jump in error: {msg}"
        );
    }

    #[test]
    fn jump_bare_short_form_no_args_required() {
        assert!(parse("jtfc").is_ok());
        assert!(parse("jolb").is_ok());
        assert!(parse("jefc").is_ok());
    }

    #[test]
    fn delimiter_scope_next_positional_rejects() {
        // cnds = Change Next Delimiter Self_ — Delimiter does not support Next
        let result = parse("cnds");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("Delimiter") || msg.contains("Next") || msg.contains("Previous"),
            "expected Delimiter/Next in error: {msg}"
        );
    }

    #[test]
    fn delimiter_scope_previous_positional_rejects() {
        // cpds = Change Previous Delimiter Self_
        let result = parse("cpds");
        assert!(result.is_err());
    }
}
