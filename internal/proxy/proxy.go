// Package proxy implements the graft egress gateway: an HTTP/HTTPS CONNECT
// proxy with a host allow-list and optional token injection per upstream.
//
// Token injection works for plain HTTP requests only. HTTPS CONNECT tunnels
// are allowed or denied based on the allow-list, but the token cannot be
// injected because the traffic is encrypted after the tunnel is established.
//
// Tokens are read from Podman secret files (/run/secrets/<name>) at startup.
// The agent container never sees the token value.
package proxy

import (
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/zerodawn1990/graft/internal/config"
)

// ConfigEnvVar is the environment variable that points to the JSON proxy
// config file inside the container (written by graft write-proxy-config).
const ConfigEnvVar = "GRAFT_PROXY_CONFIG"

// ServeFromEnv reads the proxy config from the path in GRAFT_PROXY_CONFIG
// and starts the proxy. Called inside the container by "graft proxy serve".
func ServeFromEnv() error {
	path := os.Getenv(ConfigEnvVar)
	if path == "" {
		return fmt.Errorf("proxy: %s is not set; the container must be started via graft start", ConfigEnvVar)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("proxy: reading config %s: %w", path, err)
	}
	var cfg config.ProxyConfig
	if err := json.Unmarshal(data, &cfg); err != nil {
		return fmt.Errorf("proxy: parsing config: %w", err)
	}
	return Serve(cfg)
}

// Serve starts the egress proxy described by cfg and blocks until a fatal
// error occurs.
func Serve(cfg config.ProxyConfig) error {
	level := slog.LevelInfo
	switch cfg.LogLevel {
	case "debug":
		level = slog.LevelDebug
	case "warn":
		level = slog.LevelWarn
	case "error":
		level = slog.LevelError
	}
	logger := slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: level}))

	listen := cfg.Listen
	if listen == 0 {
		listen = 8888
	}

	// Pre-load tokens from Podman secret files at startup so missing secrets
	// fail fast before any request is handled.
	tokens, err := loadTokens(cfg.Upstreams)
	if err != nil {
		return fmt.Errorf("proxy: loading secrets: %w", err)
	}

	srv := &http.Server{
		Addr: fmt.Sprintf(":%d", listen),
		Handler: &proxyHandler{
			upstreams: cfg.Upstreams,
			tokens:    tokens,
			logger:    logger,
		},
		ReadHeaderTimeout: 10 * time.Second,
	}
	logger.Info("graft proxy listening", "addr", srv.Addr, "upstreams", len(cfg.Upstreams))
	return srv.ListenAndServe()
}

type proxyHandler struct {
	upstreams []config.UpstreamConfig
	// tokens maps secret name → token value, pre-loaded from /run/secrets/.
	tokens map[string]string
	logger *slog.Logger
}

func (h *proxyHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method == http.MethodConnect {
		h.handleConnect(w, r)
	} else {
		h.handleHTTP(w, r)
	}
}

// handleConnect handles HTTPS CONNECT tunnelling (allow/deny only).
// Token injection is not possible for encrypted traffic.
func (h *proxyHandler) handleConnect(w http.ResponseWriter, r *http.Request) {
	rule := h.matchRule(r.Host)
	if rule == nil || !rule.Allow {
		h.logger.Info("DENIED", "method", "CONNECT", "target", r.Host)
		http.Error(w, "403 Forbidden", http.StatusForbidden)
		return
	}
	h.logger.Debug("ALLOWED", "method", "CONNECT", "target", r.Host)

	dst, err := net.DialTimeout("tcp", r.Host, 10*time.Second)
	if err != nil {
		http.Error(w, "502 Bad Gateway", http.StatusBadGateway)
		return
	}
	defer dst.Close()

	w.WriteHeader(http.StatusOK)
	hijacker, ok := w.(http.Hijacker)
	if !ok {
		return
	}
	conn, _, err := hijacker.Hijack()
	if err != nil {
		return
	}
	defer conn.Close()

	done := make(chan struct{}, 2)
	go func() { _, _ = io.Copy(dst, conn); done <- struct{}{} }()
	go func() { _, _ = io.Copy(conn, dst); done <- struct{}{} }()
	<-done
}

// handleHTTP handles plain HTTP proxying with optional token injection.
func (h *proxyHandler) handleHTTP(w http.ResponseWriter, r *http.Request) {
	target := r.URL.Host
	if target == "" {
		target = r.Host
	}
	rule := h.matchRule(target)
	if rule == nil || !rule.Allow {
		h.logger.Info("DENIED", "method", r.Method, "url", r.URL.String())
		http.Error(w, "403 Forbidden", http.StatusForbidden)
		return
	}
	h.logger.Debug("ALLOWED", "method", r.Method, "url", r.URL.String())

	outReq := r.Clone(r.Context())
	outReq.RequestURI = ""
	for _, hdr := range hopByHopHeaders {
		outReq.Header.Del(hdr)
	}

	// Inject token if this upstream has a secret configured.
	if rule.Secret != "" && rule.TokenHeader != "" {
		if token, ok := h.tokens[rule.Secret]; ok {
			outReq.Header.Set(rule.TokenHeader, rule.TokenPrefix+token)
		}
	}

	resp, err := http.DefaultTransport.RoundTrip(outReq)
	if err != nil {
		http.Error(w, "502 Bad Gateway", http.StatusBadGateway)
		return
	}
	defer resp.Body.Close()

	// Strip credential-related headers from the response so they never
	// reach the agent container.
	for _, hdr := range []string{"Authorization", "x-api-key", "Proxy-Authenticate"} {
		resp.Header.Del(hdr)
	}
	copyHeader(w.Header(), resp.Header)
	w.WriteHeader(resp.StatusCode)
	_, _ = io.Copy(w, resp.Body)
}

// matchRule returns the first upstream rule whose host and port match
// hostPort (e.g. "api.anthropic.com:443"). Returns nil if no rule matches.
func (h *proxyHandler) matchRule(hostPort string) *config.UpstreamConfig {
	host, port, err := net.SplitHostPort(hostPort)
	if err != nil {
		host = hostPort
		port = ""
	}
	for i := range h.upstreams {
		u := &h.upstreams[i]
		hostMatch := u.Host == "*" || strings.EqualFold(u.Host, host)
		portMatch := u.Port == 0 || fmt.Sprintf("%d", u.Port) == port
		if hostMatch && portMatch {
			return u
		}
	}
	return nil
}

// loadTokens reads Podman secret files (/run/secrets/<name>) for every
// upstream that defines a secret. Returns a map from secret name to value.
func loadTokens(upstreams []config.UpstreamConfig) (map[string]string, error) {
	tokens := make(map[string]string)
	for _, u := range upstreams {
		if u.Secret == "" || !u.Allow {
			continue
		}
		if _, ok := tokens[u.Secret]; ok {
			continue // already loaded
		}
		data, err := os.ReadFile(filepath.Join("/run/secrets", u.Secret))
		if err != nil {
			return nil, fmt.Errorf("secret %q: %w", u.Secret, err)
		}
		tokens[u.Secret] = strings.TrimSpace(string(data))
	}
	return tokens, nil
}

func copyHeader(dst, src http.Header) {
	for k, vs := range src {
		for _, v := range vs {
			dst.Add(k, v)
		}
	}
}

var hopByHopHeaders = []string{
	"Connection", "Proxy-Connection", "Keep-Alive",
	"Proxy-Authenticate", "Proxy-Authorization",
	"Te", "Trailers", "Transfer-Encoding", "Upgrade",
}
