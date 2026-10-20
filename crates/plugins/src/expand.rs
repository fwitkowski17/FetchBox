//! Turning the templates of a manifest into the arguments of a command (SPEC 6.15). The syntax is
//! parsed by `fetchbox_core::plugin::template`; this fills it in. Nothing goes through a shell: an
//! argument stays one argument whatever it contains.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fetchbox_core::plugin::template::{self, Condition, Token};
use fetchbox_core::plugin::{is_safe_relative, Arg, SettingValue};

use crate::{Error, Result};

/// Where an installed dependency is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepPaths {
    /// Its program.
    pub bin: PathBuf,
    /// Its folder.
    pub dir: PathBuf,
}

/// What templates may refer to.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The plugin's own files.
    pub plugin_dir: &'a Path,
    /// What the plugin may write (`plugin-data/<id>`).
    pub data_dir: &'a Path,
    pub tmp: &'a Path,
    /// The values of its settings (stored, else the default).
    pub settings: &'a BTreeMap<String, SettingValue>,
    /// The dependencies that are installed.
    pub deps: &'a BTreeMap<String, DepPaths>,
    pub url: Option<&'a str>,
    /// The arguments of the chosen preset.
    pub choice_args: &'a [String],
    pub out_dir: Option<&'a Path>,
}

fn run_err(msg: impl Into<String>) -> Error {
    Error::Run(msg.into())
}

/// A setting as text: a number without a pointless `.0`, a switch as `true` or `false`.
pub fn setting_text(v: Option<&SettingValue>) -> String {
    match v {
        None => String::new(),
        Some(SettingValue::Text(s)) => s.clone(),
        Some(SettingValue::Bool(b)) => b.to_string(),
        Some(SettingValue::Number(n)) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
    }
}

/// Does the setting have a value that means something: text that is not empty, a switch that is on,
/// any number.
pub fn is_set(v: Option<&SettingValue>) -> bool {
    match v {
        None => false,
        Some(SettingValue::Text(s)) => !s.trim().is_empty(),
        Some(SettingValue::Bool(b)) => *b,
        Some(SettingValue::Number(_)) => true,
    }
}

fn path_text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn dep<'a>(ctx: &'a Context<'_>, id: &str) -> Result<&'a DepPaths> {
    ctx.deps
        .get(id)
        .ok_or_else(|| run_err(format!("the dependency \"{id}\" is not installed")))
}

/// Fill in one template. `${choice.args}` is not a string, so it is an error here (see
/// [`expand_args`]).
pub fn expand_string(s: &str, ctx: &Context<'_>) -> Result<String> {
    let mut out = String::new();
    for t in template::parse(s).map_err(Error::Manifest)? {
        match t {
            Token::Literal(l) => out.push_str(&l),
            Token::Url => out.push_str(ctx.url.ok_or_else(|| run_err("there is no address here"))?),
            Token::ChoiceArgs => return Err(run_err("`${choice.args}` is a list, not text")),
            Token::Setting(k) => out.push_str(&setting_text(ctx.settings.get(&k))),
            Token::Dep(d) => out.push_str(&path_text(&dep(ctx, &d)?.bin)),
            Token::DepDir(d) => out.push_str(&path_text(&dep(ctx, &d)?.dir)),
            Token::PluginDir => out.push_str(&path_text(ctx.plugin_dir)),
            Token::DataDir => out.push_str(&path_text(ctx.data_dir)),
            Token::Tmp => out.push_str(&path_text(ctx.tmp)),
            Token::OutDir => out.push_str(&path_text(
                ctx.out_dir
                    .ok_or_else(|| run_err("there is no output folder here"))?,
            )),
        }
    }
    Ok(out)
}

fn expand_one(s: &str, ctx: &Context<'_>, out: &mut Vec<String>) -> Result<()> {
    let tokens = template::parse(s).map_err(Error::Manifest)?;
    if tokens == [Token::ChoiceArgs] {
        out.extend(ctx.choice_args.iter().cloned());
    } else {
        out.push(expand_string(s, ctx)?);
    }
    Ok(())
}

/// Fill in the arguments of a command. `${choice.args}` becomes the preset's arguments, and a
/// `{when, args}` group is dropped unless its condition holds.
pub fn expand_args(args: &[Arg], ctx: &Context<'_>) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Arg::Plain(s) => expand_one(s, ctx, &mut out)?,
            Arg::When { when, args } => {
                let holds = match template::parse_condition(when).map_err(Error::Manifest)? {
                    Condition::Setting(k) => is_set(ctx.settings.get(&k)),
                    Condition::Dep(d) => ctx.deps.contains_key(&d),
                };
                if holds {
                    for s in args {
                        expand_one(s, ctx, &mut out)?;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The program a `command` names: an installed dependency, or a file of the plugin.
pub fn resolve_program(command: &str, ctx: &Context<'_>) -> Result<PathBuf> {
    let tokens = template::parse(command).map_err(Error::Manifest)?;
    let inside = |rel: &str| {
        if is_safe_relative(rel) {
            Ok(ctx.plugin_dir.join(rel))
        } else {
            Err(run_err(format!(
                "\"{command}\" is not a path inside the plugin"
            )))
        }
    };
    match tokens.as_slice() {
        [Token::Dep(d)] => Ok(dep(ctx, d)?.bin.clone()),
        [Token::PluginDir, Token::Literal(l)] => inside(l.trim_start_matches('/')),
        [Token::Literal(l)] => inside(l),
        _ => Err(run_err(format!(
            "\"{command}\" is not a program the plugin may run"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        settings: BTreeMap<String, SettingValue>,
        deps: BTreeMap<String, DepPaths>,
    }

    fn fixture() -> Fixture {
        let mut settings = BTreeMap::new();
        settings.insert("cookies".into(), SettingValue::Text("/data/c.txt".into()));
        settings.insert("empty".into(), SettingValue::Text("  ".into()));
        settings.insert("on".into(), SettingValue::Bool(true));
        settings.insert("off".into(), SettingValue::Bool(false));
        settings.insert("n".into(), SettingValue::Number(8.0));
        settings.insert("f".into(), SettingValue::Number(1.5));
        let mut deps = BTreeMap::new();
        deps.insert(
            "tool".into(),
            DepPaths {
                bin: "/data/deps/tool/tool".into(),
                dir: "/data/deps/tool".into(),
            },
        );
        Fixture { settings, deps }
    }

    fn ctx<'a>(f: &'a Fixture, choice: &'a [String]) -> Context<'a> {
        Context {
            plugin_dir: Path::new("/plugins/demo"),
            data_dir: Path::new("/pdata/demo"),
            tmp: Path::new("/pdata/demo/tmp"),
            settings: &f.settings,
            deps: &f.deps,
            url: Some("https://example.com/watch?v=1&x=$y"),
            choice_args: choice,
            out_dir: Some(Path::new("/downloads/.job")),
        }
    }

    fn plain(s: &str) -> Arg {
        Arg::Plain(s.into())
    }

    #[test]
    fn values_are_substituted_into_single_arguments() {
        let f = fixture();
        let c = ctx(&f, &[]);
        let args = [
            plain("-J"),
            plain("--cookies=${setting:cookies}"),
            plain("${url}"),
            plain("${dep:tool}"),
            plain("${dep_dir:tool}/lib"),
            plain("${plugin_dir}/x"),
            plain("${data_dir}"),
            plain("${tmp}"),
            plain("-P${out_dir}"),
            plain("cost $$5"),
            plain("${setting:n}-${setting:f}-${setting:on}-${setting:missing}"),
        ];
        assert_eq!(
            expand_args(&args, &c).expect("expand"),
            vec![
                "-J",
                "--cookies=/data/c.txt",
                // The address stays one argument, whatever it contains.
                "https://example.com/watch?v=1&x=$y",
                "/data/deps/tool/tool",
                "/data/deps/tool/lib",
                "/plugins/demo/x",
                "/pdata/demo",
                "/pdata/demo/tmp",
                "-P/downloads/.job",
                "cost $5",
                "8-1.5-true-",
            ]
        );
    }

    #[test]
    fn the_choice_arguments_are_spliced_in() {
        let f = fixture();
        let choice = ["-f".to_owned(), "bv*+ba/b".to_owned()];
        let c = ctx(&f, &choice);
        let args = [plain("a"), plain("${choice.args}"), plain("b")];
        assert_eq!(
            expand_args(&args, &c).expect("ok"),
            ["a", "-f", "bv*+ba/b", "b"]
        );
        let none = ctx(&f, &[]);
        assert_eq!(expand_args(&args, &none).expect("ok"), ["a", "b"]);
        assert!(expand_string("${choice.args}", &c).is_err());
    }

    #[test]
    fn a_group_is_there_only_when_its_condition_holds() {
        let f = fixture();
        let c = ctx(&f, &[]);
        let when = |w: &str| Arg::When {
            when: w.into(),
            args: vec!["--flag".into(), "${setting:cookies}".into()],
        };
        let n = |w: &str| expand_args(&[when(w)], &c).expect("ok").len();
        assert_eq!(n("setting:cookies"), 2);
        assert_eq!(n("setting:on"), 2);
        assert_eq!(n("setting:n"), 2);
        assert_eq!(n("setting:off"), 0, "a switch that is off");
        assert_eq!(n("setting:empty"), 0, "text that is only blanks");
        assert_eq!(n("setting:missing"), 0);
        assert_eq!(n("dep:tool"), 2);
        assert_eq!(n("dep:ffmpeg"), 0, "not installed");
    }

    #[test]
    fn what_is_missing_is_named() {
        let f = fixture();
        let c = ctx(&f, &[]);
        let err = expand_string("${dep:ffmpeg}", &c).expect_err("missing");
        assert!(
            err.to_string().contains("\"ffmpeg\" is not installed"),
            "{err}"
        );
        let no_url = Context {
            url: None,
            out_dir: None,
            ..c
        };
        assert!(expand_string("${url}", &no_url).is_err());
        assert!(expand_string("${out_dir}", &no_url).is_err());
        assert!(expand_string("${nope}", &c).is_err());
    }

    #[test]
    fn a_program_is_a_dependency_or_a_file_of_the_plugin() {
        let f = fixture();
        let c = ctx(&f, &[]);
        let p = |s: &str| resolve_program(s, &c);
        assert_eq!(
            p("${dep:tool}").expect("dep"),
            PathBuf::from("/data/deps/tool/tool")
        );
        assert_eq!(
            p("bin/run").expect("rel"),
            PathBuf::from("/plugins/demo/bin/run")
        );
        assert_eq!(
            p("${plugin_dir}/bin/run").expect("rel"),
            PathBuf::from("/plugins/demo/bin/run")
        );
        for bad in [
            "/bin/sh",
            "../x",
            "${plugin_dir}/../x",
            "${dep:ffmpeg}",
            "${url}",
            "a b",
            "sh",
        ] {
            // `sh` is a plain relative path: a file called `sh` inside the plugin, not the system's.
            let r = p(bad);
            if bad == "sh" {
                assert_eq!(r.expect("inside"), PathBuf::from("/plugins/demo/sh"));
            } else {
                assert!(r.is_err(), "{bad}");
            }
        }
    }

    #[test]
    fn settings_read_as_text() {
        assert_eq!(setting_text(Some(&SettingValue::Number(3.0))), "3");
        assert_eq!(setting_text(Some(&SettingValue::Number(-2.0))), "-2");
        assert_eq!(setting_text(Some(&SettingValue::Number(0.25))), "0.25");
        assert_eq!(setting_text(None), "");
    }
}
