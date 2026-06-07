// Turbo VS Code extension entry point.
//
// Boots the Turbo language server (`turbolang lsp`, implemented in the
// turbo-lsp crate) and wires it to VS Code over stdio so diagnostics, hover,
// go-to-definition, completions, references, rename, and document symbols all
// work in the editor. Without this client the bundled LSP never runs and the
// extension is syntax highlighting only.

const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function startClient() {
  const config = vscode.workspace.getConfiguration("turbo");
  if (!config.get("lsp.enable", true)) {
    return;
  }

  const command = config.get("lsp.path", "turbolang");

  // `turbolang lsp` speaks LSP over stdio. The same command is used for the
  // initial run and for restarts (debug uses the same server today).
  const serverOptions = {
    run: { command, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command, args: ["lsp"], transport: TransportKind.stdio },
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

  // start() rejects if the `turbolang` binary can't be spawned — surface a
  // clear, actionable message instead of failing silently.
  client.start().catch((err) => {
    vscode.window.showErrorMessage(
      `Turbo: could not start the language server using \`${command} lsp\`. ` +
        `Install Turbo or set "turbo.lsp.path" in settings. (${err.message})`
    );
  });
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

module.exports = { activate, deactivate };
