package cli

import (
	"encoding/base64"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/zerodawn1990/graft/internal/config"
	graftproxy "github.com/zerodawn1990/graft/internal/proxy"
)

// runProxy dispatches 'graft proxy <sub>' commands.
func runProxy(args []string) error {
	if len(args) == 0 {
		return errors.New("proxy needs a subcommand: serve")
	}
	switch args[0] {
	case "serve":
		return graftproxy.ServeFromEnv()
	default:
		return fmt.Errorf("unknown proxy subcommand %q", args[0])
	}
}

// writeProxyConfig decodes a base64-encoded JSON proxy config and writes it
// to <rootfs>/run/graft-proxy.json. Called as an ExecStartPre hook by the
// systemd unit before the proxy container starts.
func writeProxyConfig(args []string) error {
	if len(args) != 2 {
		return errors.New("write-proxy-config needs: <rootfs> <base64-json>")
	}
	data, err := base64.StdEncoding.DecodeString(args[1])
	if err != nil {
		return fmt.Errorf("write-proxy-config: %w", err)
	}
	dir := filepath.Join(args[0], "run")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(dir, "graft-proxy.json"), data, 0o600)
}

// resolveProxyDep ensures the proxy service named in cfg.Proxy.Service is
// running, starting it from configRoot if needed, and injects HTTP_PROXY /
// HTTPS_PROXY / NO_PROXY into cfg.Container.Environment.
func resolveProxyDep(cfg *config.Config) error {
	if cfg.Proxy.Service == "" {
		return nil
	}
	service := cfg.Proxy.Service
	running, err := isContainerRunning(service, "")
	if err != nil {
		return err
	}
	if !running {
		configPath := filepath.Join(configRoot(), service+".toml")
		proxyFile, err := config.LoadResolved(configPath, []string{configRoot()})
		if err != nil {
			return fmt.Errorf("proxy service %q: %w", service, err)
		}
		if err := startContainer(proxyFile); err != nil {
			return fmt.Errorf("starting proxy service %q: %w", service, err)
		}
		// Give the proxy a moment to start listening.
		time.Sleep(500 * time.Millisecond)
	}
	port := cfg.Proxy.Port
	if port == 0 {
		port = 8888
	}
	if cfg.Container.Environment == nil {
		cfg.Container.Environment = map[string]string{}
	}
	proxyURL := fmt.Sprintf("http://%s:%d", service, port)
	cfg.Container.Environment["HTTP_PROXY"] = proxyURL
	cfg.Container.Environment["HTTPS_PROXY"] = proxyURL
	cfg.Container.Environment["NO_PROXY"] = "localhost,127.0.0.1"
	return nil
}
