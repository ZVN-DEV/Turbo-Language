# Turbo for VS Code

Syntax highlighting, snippets, and full language support for the
[Turbo](https://github.com/ZVN-DEV/Turbo-Language) programming language.

## Features

- Syntax highlighting (`.tb` files)
- 25 snippets for common patterns (`fn`, `struct`, `type`, `match`, `for`, `test`, …)
- Language server integration via `turbolang lsp`:
  - Diagnostics (errors and warnings with `E0NNN` codes)
  - Hover
  - Go-to-definition
  - Completions
  - References, rename, document symbols

## Requirements

The language-server features require the `turbolang` binary on your `PATH`
(install Turbo via Homebrew or build from source). If it lives elsewhere, set
`turbo.lsp.path` in your settings. To disable the server and keep only syntax
highlighting, set `turbo.lsp.enable` to `false`.

## Building / packaging locally

The client depends on `vscode-languageclient`, so install dependencies before
packaging:

```bash
cd editors/vscode/turbo-lang
npm install
npx @vscode/vsce package   # produces turbo-lang-<version>.vsix
code --install-extension turbo-lang-*.vsix
```

## Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `turbo.lsp.path` | `turbolang` | Path to the `turbolang` executable used for `turbolang lsp`. |
| `turbo.lsp.enable` | `true` | Enable the language server. |
