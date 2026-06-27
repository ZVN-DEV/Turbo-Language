#!/usr/bin/env node

const fs = require("fs");
const Module = require("module");
const path = require("path");

const repoRoot = path.resolve(__dirname, "..");
const extensionRoot = path.join(repoRoot, "editors", "vscode", "turbo-lang");
const packagePath = path.join(extensionRoot, "package.json");

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function existingContributionPath(relativePath, label) {
  assert(typeof relativePath === "string" && relativePath.length > 0, `${label} path is missing`);
  const fullPath = path.join(extensionRoot, relativePath);
  assert(fs.existsSync(fullPath), `${label} path does not exist: ${relativePath}`);
  return fullPath;
}

function validateManifest() {
  const pkg = readJson(packagePath);

  assert(pkg.name === "turbo-lang", "package name must stay turbo-lang");
  assert(pkg.main === "./extension.js", "package main must point at ./extension.js");
  assert(pkg.activationEvents?.includes("onLanguage:turbo"), "extension must activate for Turbo files");
  assert(pkg.dependencies?.["vscode-languageclient"], "extension must depend on vscode-languageclient");
  assert(pkg.contributes?.languages?.some((language) => language.id === "turbo"), "Turbo language contribution is missing");

  const config = pkg.contributes?.configuration?.properties ?? {};
  assert(config["turbo.lsp.enable"]?.default === true, "turbo.lsp.enable must default to true");
  assert(config["turbo.lsp.path"]?.default === "turbo-lsp", "turbo.lsp.path must default to turbo-lsp");
  assert(
    /formatting/i.test(config["turbo.lsp.enable"]?.description ?? ""),
    "turbo.lsp.enable description should mention formatting"
  );

  for (const language of pkg.contributes.languages ?? []) {
    if (language.configuration) {
      readJson(existingContributionPath(language.configuration, "language configuration"));
    }
  }

  for (const grammar of pkg.contributes.grammars ?? []) {
    readJson(existingContributionPath(grammar.path, "grammar"));
  }

  for (const snippet of pkg.contributes.snippets ?? []) {
    readJson(existingContributionPath(snippet.path, "snippet"));
  }

  existingContributionPath(pkg.main, "extension main");
}

function validateExtensionModule() {
  const originalLoad = Module._load;
  Module._load = function mockExtensionDependencies(request, parent, isMain) {
    if (request === "vscode") {
      return {
        workspace: {
          getConfiguration: () => ({ get: (_key, fallback) => fallback }),
          createFileSystemWatcher: () => ({}),
        },
        window: { showErrorMessage: () => undefined },
      };
    }

    if (request === "vscode-languageclient/node") {
      return {
        LanguageClient: class {
          start() {
            return Promise.resolve();
          }

          stop() {
            return Promise.resolve();
          }
        },
        TransportKind: { stdio: "stdio" },
      };
    }

    return originalLoad.call(this, request, parent, isMain);
  };

  try {
    const extension = require(path.join(extensionRoot, "extension.js"));
    assert(Array.isArray(extension.serverArgsFor("turbolang")), "serverArgsFor must return an array");
    assert(extension.serverArgsFor("turbolang").join(" ") === "lsp", "turbolang must use the lsp subcommand");
    assert(extension.serverArgsFor("turbolang.exe").join(" ") === "lsp", "turbolang.exe must use the lsp subcommand");
    assert(extension.serverArgsFor("turbo-lsp").length === 0, "turbo-lsp must run directly over stdio");
  } finally {
    Module._load = originalLoad;
  }
}

validateManifest();
validateExtensionModule();
console.log("VS Code extension smoke passed");
