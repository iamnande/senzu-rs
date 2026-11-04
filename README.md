# senzu

ever wondered what it'd be like if a crustacean ate senzu beans like they're chain-smoking fridge cigs? well wait no more...

## install

```sh
cargo install --git https://github.com/iamnande/senzu-rs
```

## config

every setting resolves flag > env > config > default. the config is `~/.config/senzu/config.toml` (or under `$XDG_CONFIG_HOME`), and every key is optional.

| setting | flag | env | config | default |
|---|---|---|---|---|
| beans dir | `--beans` | `SENZU_BEANS` | `beans` | none, senzu asks for one |
| agent | `--agent` | `SENZU_AGENT` | `agent` | `claude` |
| launch | `--launch` | `SENZU_LAUNCH` | `launch` | `zellij` inside zellij, else `exec` |

```toml
beans = "~/path/to/beans"
```

logs go to `~/.local/state/senzu/` (or under `$XDG_STATE_HOME`), a file a day, a week kept. `RUST_LOG` sets the level.

## use

```
senzu                 tui on a tty, the current stream otherwise
senzu ls [--all]      open streams, urgent and high first
senzu show <slug>     one bean
senzu open <slug>     launch the agent in the bean's worktree
senzu check           validate every bean
```

## license

agpl-3.0-only, see [LICENSE](LICENSE).
