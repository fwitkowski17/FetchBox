//! Argument templates. Commands are never run through a shell, so a template only substitutes
//! values into single arguments:
//!
//! `${url}`, `${choice.args}` (splices the arguments of the chosen preset; the whole argument),
//! `${setting:key}`, `${dep:id}` (the dependency's program), `${dep_dir:id}` (its folder),
//! `${plugin_dir}`, `${data_dir}`, `${tmp}`, `${out_dir}` and `$$` for a `$`.

/// One piece of a parsed template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Literal(String),
    Url,
    /// `${choice.args}`: the argument list of the chosen preset.
    ChoiceArgs,
    Setting(String),
    Dep(String),
    DepDir(String),
    PluginDir,
    DataDir,
    Tmp,
    OutDir,
}

/// A condition of a `{when, args}` group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// The setting has a value (not empty, not off).
    Setting(String),
    /// The dependency is installed.
    Dep(String),
}

/// Parse a template. Errors say what is wrong, for the author.
pub fn parse(template: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut chars = template.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '$' {
            lit.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some((_, '$')) => {
                chars.next();
                lit.push('$');
            }
            Some((_, '{')) => {
                chars.next();
                let rest = &template[i + 2..];
                let end = rest
                    .find('}')
                    .ok_or_else(|| format!("`${{` is not closed in \"{template}\""))?;
                let name = &rest[..end];
                if !lit.is_empty() {
                    out.push(Token::Literal(std::mem::take(&mut lit)));
                }
                out.push(token(name, template)?);
                // Skip the name and the closing brace.
                for _ in 0..name.chars().count() + 1 {
                    chars.next();
                }
            }
            _ => {
                return Err(format!(
                    "a lone `$` in \"{template}\" (write `$$` for a dollar sign)"
                ))
            }
        }
    }
    if !lit.is_empty() {
        out.push(Token::Literal(lit));
    }
    if out.contains(&Token::ChoiceArgs) && out.len() != 1 {
        return Err(format!(
            "`${{choice.args}}` must be an argument of its own, not part of \"{template}\""
        ));
    }
    Ok(out)
}

fn token(name: &str, template: &str) -> Result<Token, String> {
    let key = |what: &str, v: &str| -> Result<String, String> {
        if v.is_empty() {
            Err(format!("`${{{what}:}}` needs a name in \"{template}\""))
        } else {
            Ok(v.to_owned())
        }
    };
    Ok(match name.split_once(':') {
        Some(("setting", v)) => Token::Setting(key("setting", v)?),
        Some(("dep", v)) => Token::Dep(key("dep", v)?),
        Some(("dep_dir", v)) => Token::DepDir(key("dep_dir", v)?),
        Some(_) => return Err(format!("unknown `${{{name}}}` in \"{template}\"")),
        None => match name {
            "url" => Token::Url,
            "choice.args" => Token::ChoiceArgs,
            "plugin_dir" => Token::PluginDir,
            "data_dir" => Token::DataDir,
            "tmp" => Token::Tmp,
            "out_dir" => Token::OutDir,
            _ => return Err(format!("unknown `${{{name}}}` in \"{template}\"")),
        },
    })
}

/// Parse the condition of a `{when, args}` group: `setting:key` or `dep:id`.
pub fn parse_condition(when: &str) -> Result<Condition, String> {
    match when.split_once(':') {
        Some(("setting", k)) if !k.is_empty() => Ok(Condition::Setting(k.to_owned())),
        Some(("dep", d)) if !d.is_empty() => Ok(Condition::Dep(d.to_owned())),
        _ => Err(format!(
            "the condition \"{when}\" must be `setting:<key>` or `dep:<id>`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_is_split_into_literals_and_values() {
        assert_eq!(
            parse("plain").unwrap(),
            vec![Token::Literal("plain".into())]
        );
        assert_eq!(
            parse("--out=${out_dir}/x").unwrap(),
            vec![
                Token::Literal("--out=".into()),
                Token::OutDir,
                Token::Literal("/x".into())
            ]
        );
        assert_eq!(
            parse("${dep:yt-dlp}").unwrap(),
            vec![Token::Dep("yt-dlp".into())]
        );
        assert_eq!(
            parse("${setting:cookies_file}").unwrap(),
            vec![Token::Setting("cookies_file".into())]
        );
        assert_eq!(parse("${url}").unwrap(), vec![Token::Url]);
        assert_eq!(parse("").unwrap(), Vec::new());
    }

    #[test]
    fn a_dollar_sign_is_written_twice() {
        assert_eq!(parse("a$$b").unwrap(), vec![Token::Literal("a$b".into())]);
        assert_eq!(
            parse("$${url}").unwrap(),
            vec![Token::Literal("${url}".into())]
        );
        assert!(parse("a$b").is_err());
        assert!(parse("cost $").is_err());
    }

    #[test]
    fn mistakes_are_named() {
        assert!(parse("${url").unwrap_err().contains("not closed"));
        assert!(parse("${nope}").unwrap_err().contains("unknown"));
        assert!(parse("${setting:}").unwrap_err().contains("needs a name"));
        assert!(parse("${other:x}").unwrap_err().contains("unknown"));
    }

    #[test]
    fn the_choice_arguments_are_an_argument_of_their_own() {
        assert_eq!(parse("${choice.args}").unwrap(), vec![Token::ChoiceArgs]);
        assert!(parse("x${choice.args}").is_err());
    }

    #[test]
    fn multibyte_text_survives() {
        assert_eq!(
            parse("zażółć ${url} gęślą").unwrap(),
            vec![
                Token::Literal("zażółć ".into()),
                Token::Url,
                Token::Literal(" gęślą".into())
            ]
        );
    }

    #[test]
    fn conditions_name_a_setting_or_a_dependency() {
        assert_eq!(
            parse_condition("setting:cookies_file").unwrap(),
            Condition::Setting("cookies_file".into())
        );
        assert_eq!(
            parse_condition("dep:ffmpeg").unwrap(),
            Condition::Dep("ffmpeg".into())
        );
        assert!(parse_condition("ffmpeg").is_err());
        assert!(parse_condition("dep:").is_err());
        assert!(parse_condition("os:linux").is_err());
    }
}
