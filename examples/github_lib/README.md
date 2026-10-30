# A library from GitHub

[microaitec-deploy/fogulang_module](https://github.com/microaitec-deploy/fogulang_module)
is a small Fogu library (greetings, a slug helper, a `Counter` struct). This project
depends on it through `fogu.json`:

```sh
fogu install          # fetches it into ~/.fogu/fogu_modules and pins fogu.lock
fogu main.fg
```

To add such a dependency to your own project: `fogu add microaitec-deploy/fogulang_module`
(or `github:…`, `gitlab:…`, a git URL, `user/repo@^0.1.0` for a tag range). How to write
and publish a library: `docs/packages.md`.
