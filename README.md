# senzu

ever wondered what it'd be like if a crustacean ate senzu beans like they're chain-smoking fridge cigs? well wait no more...

## install

```sh
cargo install --git https://github.com/iamnande/senzu-rs
```

## beans

a bean is one stream of work, a `<slug>.json` in the beans dir. `senzu check` says what the schema wants. a bean's rfc sits beside it as `<slug>.rfc.md`: found there, never listed in the bean, and kept out of git. `check` fails on an rfc git tracks, and warns on one with no bean.

## config

every setting resolves flag > env > config > default. the config is `~/.config/senzu/config.toml` (or under `$XDG_CONFIG_HOME`). no key has to be in it, but whatever is in it has to be valid: an unknown key, or an `agent` or `launch` senzu doesn't know, stops every command with exit 2. `senzu config` shows each value and the layer that set it.

| setting | flag | env | config | default |
|---|---|---|---|---|
| beans dir | `--beans` | `SENZU_BEANS` | `beans` | none: every command but `config` stops until one is set |
| agent | `--agent` | `SENZU_AGENT` | `agent` | `claude` |
| launch | `--launch` | `SENZU_LAUNCH` | `launch` | `zellij` inside zellij, else `exec` |
| state dir | `--state` | `SENZU_STATE` | none, the log starts before the config | `$XDG_STATE_HOME/senzu`, else `~/.local/state/senzu` |
| worktrees | `--worktrees` | `SENZU_WORKTREES` | `worktrees` | `~/worktrees` |
| branch prefix | `--branch-prefix` | `SENZU_BRANCH_PREFIX` | `branch_prefix` | none, no prefix |
| thread channel | `--thread-channel` | `SENZU_THREAD_CHANNEL` | `thread_channel` | none, no slack |

senzu itself reads the first four. the last three are for the skills, which ask `senzu config --json` instead of hardcoding them. an empty value counts as unset.

```toml
beans = "~/path/to/beans"
worktrees = "~/worktrees"
branch_prefix = "crab/"
```

logs go to the state dir, a file a day, a week kept. `RUST_LOG` sets the level.

## use

```
senzu                 tui on a tty, the current stream otherwise
senzu ls [--all]      open streams, urgent and high first
senzu show <slug>     one bean
senzu open <slug>     launch the agent on the bean's next skill, from the beans dir's parent
senzu check           validate every bean
senzu config          each setting and where it came from
```

piped, or with `--json`, `ls`, `show` and `config` print json. paths come out plain and absolute, so they paste straight into a command.

`senzu show <slug> --json`:

```json
{
  "bean": {},
  "path": "/home/crab/beans/crab.json",
  "attachments": [
    { "kind": "rfc", "path": "/home/crab/beans/crab.rfc.md" },
    { "kind": "issue", "url": "https://example.com/issues/1" }
  ]
}
```

kinds are `rfc`, `artifact`, `issue` and `link`. anything that starts with `scheme://` has `url`, the rest is a local file with `path` (so `mailto:` is a path).

`senzu config --json`, one row per setting, unset is `null`:

```json
[
  { "key": "beans", "value": "/home/crab/beans", "from": "env:SENZU_BEANS" },
  { "key": "thread_channel", "value": null, "from": "default:" }
]
```

`from` is the layer: `flag:--beans`, `env:SENZU_BEANS`, `file:///home/crab/.config/senzu/config.toml` or `default:`.

## exit codes

`0` ok, `1` something failed (like `check` finding a problem), `2` a usage or config mistake, `3` no bean by that slug.

## license

`AGPL-3.0-only`, see [LICENSE](LICENSE).
