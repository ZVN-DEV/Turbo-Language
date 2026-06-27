// Turbo VS Code extension entry point.
//
// Boots the Turbo language server (`turbo-lsp`) and wires it to VS Code over
// stdio so diagnostics, hover,
// go-to-definition, completions, references, rename, and document symbols all
// work in the editor. Without this client the bundled LSP never runs and the
// extension is syntax highlighting only.

const vscode = require("vscode");
const path = require("path");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function startClient() {
  const config = vscode.workspace.getConfiguration("turbo");
  if (!config.get("lsp.enable", true)) {
    return;
  }

  const command = config.get("lsp.path", "turbo-lsp");
  const args = serverArgsFor(command);

  // `turbo-lsp` speaks LSP over stdio directly. Older installs can still set
  // the path to `turbolang`, which needs the `lsp` subcommand.
  const serverOptions = {
    run: { command, args, transport: TransportKind.stdio },
    debug: { command, args, transport: TransportKind.stdio },
  };

  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "turbo" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.tb"),
    },
  };

  client = new LanguageClient(
    "turbo",
    "Turbo Language Server",
    serverOptions,
    clientOptions
  );

  // start() rejects if the server binary can't be spawned — surface a
  // clear, actionable message instead of failing silently.
  client.start().catch((err) => {
    const renderedCommand = [command, ...args].join(" ");
    vscode.window.showErrorMessage(
      `Turbo: could not start the language server using \`${renderedCommand}\`. ` +
        `Install Turbo or set "turbo.lsp.path" in settings. (${err.message})`
    );
  });
}

function serverArgsFor(command) {
  const base = path.basename(command).replace(/\.exe$/i, "");
  return base === "turbolang" ? ["lsp"] : [];
}

function activate(_context) {
  startClient();
}

function deactivate() {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

module.exports = { activate, deactivate, serverArgsFor };
