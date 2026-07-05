package cli

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/Patrick-Kappen/graft/internal/config"
	"github.com/Patrick-Kappen/graft/internal/quadlet"
	graftruntime "github.com/Patrick-Kappen/graft/internal/runtime"
)

func renderYAML(args []string) error {
	if len(args) != 1 {
		return errors.New("render needs exactly one TOML file")
	}
	file, err := config.Load(args[0])
	if err != nil {
		return err
	}
	if file.IsNoop() {
		return nil
	}
	if err := rejectUnresolvedGraphFeatures(file); err != nil {
		return err
	}
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	resolvedArgs, err := resolveRuntimeCommand(file.Config.Runtime.Command, file.Config.Runtime.Packages)
	if err != nil {
		return err
	}
	renderConfig := withRuntimePathEnv(file.Config, resolvedArgs)
	text, err := quadlet.RenderRootfsContainer(quadlet.RenderInput{
		Rootfs:                "<runtime-rootfs>",
		FallbackContainerName: "<runtime-container-name>",
		Command:               resolvedArgs,
		Config:                renderConfig,
	})
	if err != nil {
		return err
	}
	fmt.Print(text)
	return nil
}

func renderNixOS(args []string) error {
	if len(args) != 3 {
		return errors.New("render-nixos needs: <file.toml> <rootfs> <container-name>")
	}
	file, err := config.Load(args[0])
	if err != nil {
		return err
	}
	if file.IsNoop() {
		return nil
	}
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	resolvedArgs, err := resolveHostStoreCommand(file.Config.Runtime.Command)
	if err != nil {
		return err
	}
	renderConfig := withRuntimePathEnv(file.Config, resolvedArgs)
	text, err := quadlet.RenderRootfsContainer(quadlet.RenderInput{
		Rootfs:                args[1],
		FallbackContainerName: args[2],
		Command:               resolvedArgs,
		Config:                renderConfig,
	})
	if err != nil {
		return err
	}
	fmt.Print(text)
	return nil
}

func renderNixOSUnits(args []string) error {
	if len(args) != 3 {
		return errors.New("render-nixos-units needs: <file.toml> <container-name> <out-dir>")
	}
	file, err := config.Load(args[0])
	if err != nil {
		return err
	}
	if file.IsNoop() {
		return nil
	}
	if err := validateRunnable(file.Config.Runtime); err != nil {
		return err
	}
	resolvedArgs, err := resolveHostStoreCommand(file.Config.Runtime.Command)
	if err != nil {
		return err
	}
	name := args[1]
	outDir := args[2]

	graftBin, err := os.Executable()
	if err != nil {
		return err
	}
	runtimeRootfs := managedRootfsPath(name)
	renderConfig := withRuntimePathEnv(file.Config, resolvedArgs)
	units, err := quadlet.RenderRootfsUnits(quadlet.RenderInput{
		Rootfs:                runtimeRootfs,
		FallbackContainerName: name,
		Command:               resolvedArgs,
		Config:                renderConfig,
		RootfsPrepare:         []string{graftBin, "prepare-rootfs", runtimeRootfs},
	})
	if err != nil {
		return err
	}
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		return err
	}
	for _, unit := range units {
		if err := os.WriteFile(filepath.Join(outDir, unit.Name), []byte(unit.Text), 0o644); err != nil {
			return err
		}
	}
	return nil
}

// managedRootfsPath is the writable rootfs location for a module-managed unit.
// The %t token is expanded by systemd at runtime (per-user or system runtime dir).
func managedRootfsPath(name string) string {
	return "%t/graft/" + name + "/rootfs"
}

func prepareRootfs(args []string) error {
	if len(args) != 1 {
		return errors.New("prepare-rootfs needs exactly one target directory")
	}
	return graftruntime.CreateMinimalRootfs(args[0])
}
