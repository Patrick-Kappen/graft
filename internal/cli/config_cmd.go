package cli

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/Patrick-Kappen/graft/internal/config"
)

func runConfig(args []string) error {
	if len(args) == 0 {
		return errors.New("config needs a subcommand: path, init, show")
	}
	switch args[0] {
	case "path":
		fmt.Println(defaultConfigPath())
		return nil
	case "init":
		path := defaultConfigPath()
		if len(args) > 1 {
			path = args[1]
		}
		return initConfig(path)
	case "show":
		path := defaultConfigPath()
		if len(args) > 1 {
			path = args[1]
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		fmt.Print(string(data))
		return nil
	default:
		return fmt.Errorf("unknown config subcommand %q", args[0])
	}
}

func defaultConfigPath() string {
	if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
		return filepath.Join(xdg, "graft", "config.toml")
	}
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		return "config.toml"
	}
	return filepath.Join(home, ".config", "graft", "config.toml")
}

func initConfig(path string) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	if _, err := os.Stat(path); err == nil {
		return fmt.Errorf("config already exists: %s", path)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return os.WriteFile(path, []byte(exampleConfig), 0o644)
}

const exampleConfig = `# graft config
#
# Empty means no-op. This template intentionally does not define containers,
# presets, mounts, runtimes, or security policy.

version = 1
name = "example"

[parents]
add = []
remove = []
set = []

[children]
add = []
remove = []
set = []

[config]
# Empty means no-op.
`

type inspectOutput struct {
	Version     int    `json:"version"`
	Name        string `json:"name,omitempty"`
	Noop        bool   `json:"noop"`
	RuntimeMode string `json:"runtimeMode,omitempty"`
}

func inspectYAML(args []string) error {
	if len(args) != 1 {
		return errors.New("inspect needs exactly one TOML file")
	}
	file, err := config.Load(args[0])
	if err != nil {
		return err
	}
	output := inspectOutput{
		Version:     file.Version,
		Name:        file.Name,
		Noop:        file.IsNoop(),
		RuntimeMode: file.Config.Runtime.Mode,
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	return encoder.Encode(output)
}

func autodetectConfig() (string, error) {
	candidates := []string{"graft.toml", ".graft.toml", "config.toml"}
	for _, candidate := range candidates {
		info, err := os.Stat(candidate)
		if err == nil && !info.IsDir() {
			return candidate, nil
		}
		if err != nil && !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
	}
	return "", errors.New("no TOML config found; tried graft.toml, .graft.toml, config.toml")
}
