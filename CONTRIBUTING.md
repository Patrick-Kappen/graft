# Contributing

Thanks for considering a contribution.

## Development checks

Run the standard checks before sending changes:

```bash
nix develop -c gofmt -w cmd internal
nix develop -c nixfmt flake.nix nix
nix develop -c go test ./...
nix develop -c go vet ./...
nix develop -c golangci-lint run ./...
nix develop -c statix check .
nix develop -c deadnix .
nix flake check --no-build
```

## Design rules

- TOML is user-authored source of truth.
- Empty config is no-op.
- No implicit containers, mounts, presets, or security policy.
- Secrets must not enter TOML or the Nix store.
- Prefer typed schema fields over raw Quadlet passthrough when adding common features.
- Keep CLI and NixOS/Home Manager behavior aligned; if not aligned yet, fail loudly.
