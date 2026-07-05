package cli

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/Patrick-Kappen/graft/internal/config"
)

// nixBake reads package.json and package-lock.json from a directory, prefetches
// the npm deps hash using prefetch-npm-deps (from nixpkgs), and emits a
// buildNpmPackage Nix snippet ready to paste into a flake.
func nixBake(args []string) error {
	if len(args) != 1 {
		return errors.New("nix-bake needs: <dir>")
	}
	dir, err := filepath.Abs(args[0])
	if err != nil {
		return err
	}

	pkgData, err := os.ReadFile(filepath.Join(dir, "package.json"))
	if err != nil {
		return fmt.Errorf("reading package.json: %w", err)
	}
	var pkg struct {
		Name    string `json:"name"`
		Version string `json:"version"`
		Main    string `json:"main"`
	}
	if err := json.Unmarshal(pkgData, &pkg); err != nil {
		return fmt.Errorf("parsing package.json: %w", err)
	}
	if pkg.Name == "" {
		pkg.Name = "my-agent"
	}
	if pkg.Version == "" {
		pkg.Version = "0.0.1"
	}

	lockFile := filepath.Join(dir, "package-lock.json")
	if _, err := os.Stat(lockFile); err != nil {
		return fmt.Errorf("package-lock.json not found in %s; run \"npm install\" first", dir)
	}

	_, _ = fmt.Fprintf(os.Stderr, "graft: prefetching npm deps hash for %s@%s...\n", pkg.Name, pkg.Version)
	hash, err := prefetchNpmDeps(lockFile)
	if err != nil {
		return fmt.Errorf(
			"prefetch-npm-deps failed: %w\n\nRun manually and copy the hash:\n  nix run nixpkgs#prefetch-npm-deps -- %s",
			err, lockFile,
		)
	}

	nixName := nixIdentifier(pkg.Name)
	relDir := filepath.Base(dir)

	var b strings.Builder
	b.WriteString("# --- graft nix-bake output ---\n")
	b.WriteString("# Paste into your flake.nix or a dedicated .nix file.\n")
	b.WriteString("#\n")
	b.WriteString("# 1. Add the package to your flake packages:\n")
	b.WriteString("#      " + nixName + " = pkgs.callPackage ./" + relDir + ".nix { };\n")
	b.WriteString("#\n")
	b.WriteString("# 2. Reference it in your graft TOML:\n")
	b.WriteString("#      [config.runtime]\n")
	b.WriteString("#      packages = [" + fmt.Sprintf("%q", nixName) + "]\n")
	b.WriteString("#\n\n")
	b.WriteString("{ pkgs, ... }:\n\n")
	b.WriteString("pkgs.buildNpmPackage {\n")
	b.WriteString("  pname = " + fmt.Sprintf("%q", pkg.Name) + ";\n")
	b.WriteString("  version = " + fmt.Sprintf("%q", pkg.Version) + ";\n")
	b.WriteString("  src = ./" + relDir + ";\n")
	b.WriteString("  npmDepsHash = " + fmt.Sprintf("%q", hash) + ";\n")
	b.WriteString("\n")
	b.WriteString("  # If your package exposes a binary, add an installPhase:\n")
	b.WriteString("  # installPhase = ''\n")
	b.WriteString("  #   mkdir -p $out/bin $out/lib/node_modules/" + pkg.Name + "\n")
	b.WriteString("  #   cp -r . $out/lib/node_modules/" + pkg.Name + "\n")
	b.WriteString("  #   makeWrapper ${pkgs.nodejs}/bin/node $out/bin/" + nixName + " \\\n")
	b.WriteString("  #     --add-flags \"$out/lib/node_modules/" + pkg.Name + "/" + pkg.Main + "\"\n")
	b.WriteString("  # ''\n")
	b.WriteString("}\n")

	fmt.Print(b.String())
	return nil
}

// prefetchNpmDeps runs prefetch-npm-deps on the given package-lock.json and
// returns the trimmed hash string. It tries the tool from PATH first and falls
// back to nix run.
func prefetchNpmDeps(lockFile string) (string, error) {
	if path, err := exec.LookPath("prefetch-npm-deps"); err == nil {
		out, err := exec.Command(path, lockFile).Output()
		if err != nil {
			return "", err
		}
		return strings.TrimSpace(string(out)), nil
	}
	out, err := exec.Command("nix", "run", "--impure", "nixpkgs#prefetch-npm-deps", "--", lockFile).Output()
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(out)), nil
}

// nixIdentifier converts an npm package name to a valid Nix identifier.
// Examples: @anthropic-ai/sdk → anthropic-ai-sdk, my.package → my-package
func nixIdentifier(name string) string {
	name = strings.TrimPrefix(name, "@")
	name = strings.ReplaceAll(name, "/", "-")
	var b strings.Builder
	for _, r := range name {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9', r == '-', r == '_':
			b.WriteRune(r)
		default:
			b.WriteRune('-')
		}
	}
	return b.String()
}

func withRuntimePathEnv(cfg config.Config, args []string) config.Config {
	if len(args) == 0 || !strings.HasPrefix(args[0], "/nix/store/") {
		return cfg
	}
	out := cfg
	if out.Container.Environment == nil {
		out.Container.Environment = map[string]string{}
	}
	if _, ok := out.Container.Environment["PATH"]; !ok {
		out.Container.Environment["PATH"] = filepath.Dir(args[0])
	}
	return out
}

func rejectUnresolvedGraphFeatures(file *config.File) error {
	if len(file.Parents.Add) > 0 || len(file.Parents.Set) > 0 || len(file.Parents.Remove) > 0 ||
		len(file.Children.Add) > 0 || len(file.Children.Set) > 0 || len(file.Children.Remove) > 0 {
		return errors.New("direct CLI run/render does not resolve parents/children yet; use NixOS/Home Manager configRoot or an effective TOML")
	}
	ops := file.Config.Runtime.PackageOps
	if len(ops.Add) > 0 || len(ops.Remove) > 0 || len(ops.Replace) > 0 {
		return errors.New("direct CLI run/render does not apply config.runtime.packageOps yet; use NixOS/Home Manager configRoot or an effective TOML")
	}
	return nil
}

func validateRunnable(runtime config.RuntimeConfig) error {
	if runtime.Mode != "rootfs-store" {
		return fmt.Errorf("config.runtime.mode must be rootfs-store for runnable configs, got %q", runtime.Mode)
	}
	if len(runtime.Command) == 0 {
		return errors.New("config.runtime.command must not be empty for runnable configs")
	}
	return nil
}

func resolveHostStoreCommand(args []string) ([]string, error) {
	if len(args) == 0 {
		return nil, errors.New("command is required")
	}
	if filepath.IsAbs(args[0]) {
		return args, nil
	}
	return resolveFromHostPath(args)
}

func resolveRuntimeCommand(args []string, packageNames []string) ([]string, error) {
	if len(args) == 0 {
		return nil, errors.New("command is required")
	}
	if filepath.IsAbs(args[0]) {
		return args, nil
	}
	if len(packageNames) > 0 {
		runtimeEnv, err := buildRuntimeEnv(packageNames)
		if err != nil {
			return nil, err
		}
		candidate := filepath.Join(runtimeEnv, "bin", args[0])
		if info, err := os.Stat(candidate); err == nil && !info.IsDir() {
			return append([]string{candidate}, args[1:]...), nil
		}
		return nil, fmt.Errorf("command %q not found in runtime packages at %s/bin", args[0], runtimeEnv)
	}
	return resolveFromHostPath(args)
}

// resolveFromHostPath resolves a non-absolute command via the host PATH and
// requires it to live inside /nix/store so runs never depend on host-installed
// binaries outside the Nix store.
func resolveFromHostPath(args []string) ([]string, error) {
	path, err := exec.LookPath(args[0])
	if err != nil {
		return nil, fmt.Errorf("command %q not found on host PATH; use an absolute /nix/store path or set config.runtime.packages", args[0])
	}
	if !strings.HasPrefix(path, "/nix/store/") {
		return nil, fmt.Errorf("command %q resolved to %s, which is not inside /nix/store; set config.runtime.packages or use an absolute /nix/store path", args[0], path)
	}
	return append([]string{path}, args[1:]...), nil
}

var packageNamePattern = regexp.MustCompile(`^[A-Za-z0-9._+-]+$`)

func buildRuntimeEnv(packageNames []string) (string, error) {
	for _, packageName := range packageNames {
		if !packageNamePattern.MatchString(packageName) {
			return "", fmt.Errorf("invalid package name %q; expected only letters, numbers, dot, underscore, plus, or dash", packageName)
		}
	}
	expr := `let
  pkgs = import (builtins.getFlake "nixpkgs") { system = builtins.currentSystem; };
  packageNames = [` + nixStringList(packageNames) + ` ];
  packages = map (name:
    if builtins.hasAttr name pkgs then builtins.getAttr name pkgs
    else throw "unknown package ${name}"
  ) packageNames;
in pkgs.buildEnv { name = "graft-runtime"; paths = packages; }`
	cmd := exec.Command("nix", "build", "--no-link", "--print-out-paths", "--impure", "--expr", expr)
	out, err := cmd.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("failed to build runtime closure with nix: %w\n%s", err, strings.TrimSpace(string(out)))
	}
	path := strings.TrimSpace(string(out))
	if path == "" {
		return "", errors.New("nix build returned an empty runtime path")
	}
	if fields := strings.Fields(path); len(fields) > 0 {
		path = fields[len(fields)-1]
	}
	return path, nil
}

func nixStringList(values []string) string {
	quoted := make([]string, len(values))
	for i, value := range values {
		quoted[i] = nixString(value)
	}
	return strings.Join(quoted, " ")
}

func nixString(value string) string {
	value = strings.ReplaceAll(value, `\`, `\\`)
	value = strings.ReplaceAll(value, `"`, `\"`)
	value = strings.ReplaceAll(value, `${`, `\${`)
	return `"` + value + `"`
}
