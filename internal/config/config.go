package config

type File struct {
	Version    int              `toml:"version"`
	Name       string           `toml:"name"`
	Parents    RelationSet      `toml:"parents"`
	Children   RelationSet      `toml:"children"`
	Deploy     DeployConfig     `toml:"deploy"`
	Validation ValidationConfig `toml:"validation"`
	Config     Config           `toml:"config"`
}

type RelationSet struct {
	Add    []string `toml:"add"`
	Remove []string `toml:"remove"`
	Set    []string `toml:"set"`
}

type DeployConfig struct {
	Enable bool   `toml:"enable"`
	Target string `toml:"target"`
}

type ValidationConfig struct {
	Level string `toml:"level"`
}

type Config struct {
	Runtime    RuntimeConfig       `toml:"runtime"`
	Container  ContainerConfig     `toml:"container"`
	Filesystem FilesystemConfig    `toml:"filesystem"`
	Network    NetworkConfig       `toml:"network"`
	Networks   []NetworkUnitConfig `toml:"networks"`
	Volumes    []VolumeUnitConfig  `toml:"volumes"`
	Security   SecurityConfig      `toml:"security"`
	Resources  ResourcesConfig     `toml:"resources"`
	Secrets    []SecretConfig      `toml:"secrets"`
	Workspace  WorkspaceConfig     `toml:"workspace"`
	Home       HomeConfig          `toml:"home"`
	Attach     AttachConfig        `toml:"attach"`
	Proxy      ProxyConfig         `toml:"proxy"`
	Service    ServiceConfig       `toml:"service"`
	Quadlet    QuadletConfig       `toml:"quadlet"`
}

type RuntimeConfig struct {
	Mode       string           `toml:"mode"`
	Packages   []string         `toml:"packages"`
	PackageOps PackageOpsConfig `toml:"packageOps"`
	Command    []string         `toml:"command"`
}

type PackageOpsConfig struct {
	Add     []string               `toml:"add"`
	Remove  []string               `toml:"remove"`
	Replace []PackageReplaceConfig `toml:"replace"`
}

type PackageReplaceConfig struct {
	Name string `toml:"name"`
	With string `toml:"with"`
}

type ContainerConfig struct {
	// Identity
	Name     string `toml:"name"`
	Hostname string `toml:"hostname"`
	Pod      string `toml:"pod"`

	// Execution
	Entrypoint  []string `toml:"entrypoint"`
	StopSignal  string   `toml:"stopSignal"`
	StopTimeout int      `toml:"stopTimeout"`
	WorkingDir  string   `toml:"workingDir"`
	User        string   `toml:"user"`
	Group       string   `toml:"group"`
	Timezone    string   `toml:"timezone"`
	Notify      string   `toml:"notify"`
	RunInit     *bool    `toml:"runInit"`

	// Environment
	Annotations     map[string]string `toml:"annotations"`
	Environment     map[string]string `toml:"environment"`
	EnvironmentFile []string          `toml:"environmentFile"`
	EnvironmentHost *bool             `toml:"environmentHost"`

	// Podman args
	PodmanArgs []string `toml:"podmanArgs"`
	GlobalArgs []string `toml:"globalArgs"`

	// Network identity
	IP             string   `toml:"ip"`
	IP6            string   `toml:"ip6"`
	NetworkAlias   []string `toml:"networkAlias"`
	ExposeHostPort []string `toml:"exposeHostPort"`

	// User namespace
	UIDMap    []string `toml:"uidMap"`
	GIDMap    []string `toml:"gidMap"`
	SubUIDMap string   `toml:"subUidMap"`
	SubGIDMap string   `toml:"subGidMap"`

	// Filesystem extras
	ShmSize     string   `toml:"shmSize"`
	Mask        []string `toml:"mask"`
	UnmaskPaths []string `toml:"unmaskPaths"`
	Sysctl      []string `toml:"sysctl"`

	// Logging
	LogDriver string `toml:"logDriver"`

	// Health checks
	Health HealthConfig `toml:"health"`
}

// HealthConfig describes the container health check (Quadlet HealthCmd et al.).
type HealthConfig struct {
	Cmd             string `toml:"cmd"`
	Interval        string `toml:"interval"`
	Timeout         string `toml:"timeout"`
	Retries         int    `toml:"retries"`
	StartPeriod     string `toml:"startPeriod"`
	OnFailure       string `toml:"onFailure"`
	StartupCmd      string `toml:"startupCmd"`
	StartupInterval string `toml:"startupInterval"`
	StartupRetries  int    `toml:"startupRetries"`
	StartupSuccess  int    `toml:"startupSuccess"`
	StartupTimeout  string `toml:"startupTimeout"`
}

type FilesystemConfig struct {
	ReadOnly      *bool          `toml:"readOnly"`
	ReadOnlyTmpfs *bool          `toml:"readOnlyTmpfs"`
	Tmpfs         []string       `toml:"tmpfs"`
	Volumes       []VolumeConfig `toml:"volumes"`
	Mounts        []string       `toml:"mounts"`
	Devices       []DeviceConfig `toml:"devices"`
}

type VolumeConfig struct {
	Source string `toml:"source"`
	Target string `toml:"target"`
	Mode   string `toml:"mode"`
}

type DeviceConfig struct {
	Source      string `toml:"source"`
	Target      string `toml:"target"`
	Permissions string `toml:"permissions"`
}

type NetworkConfig struct {
	// Mode is a single network name (e.g. "none", "pasta", "foo.network").
	// Use Modes when a container needs more than one network.
	Mode  string   `toml:"mode"`
	Modes []string `toml:"modes"`

	Publish   []string `toml:"publish"`
	DNS       []string `toml:"dns"`
	DNSOption []string `toml:"dnsOption"`
	DNSSearch []string `toml:"dnsSearch"`
	AddHost   []string `toml:"addHost"`
}

type NetworkUnitConfig struct {
	Name     string              `toml:"name"`
	Driver   string              `toml:"driver"`
	Internal *bool               `toml:"internal"`
	IPv6     *bool               `toml:"ipv6"`
	Subnet   string              `toml:"subnet"`
	Gateway  string              `toml:"gateway"`
	IPRange  string              `toml:"ipRange"`
	DNS      []string            `toml:"dns"`
	Options  []string            `toml:"options"`
	Labels   map[string]string   `toml:"labels"`
	Quadlet  map[string][]string `toml:"quadlet"`
}

type VolumeUnitConfig struct {
	Name    string              `toml:"name"`
	Driver  string              `toml:"driver"`
	Copy    *bool               `toml:"copy"`
	Options []string            `toml:"options"`
	Labels  map[string]string   `toml:"labels"`
	Quadlet map[string][]string `toml:"quadlet"`
}

type SecurityConfig struct {
	DropCapabilities      []string `toml:"dropCapabilities"`
	AddCapabilities       []string `toml:"addCapabilities"`
	NoNewPrivileges       *bool    `toml:"noNewPrivileges"`
	Privileged            *bool    `toml:"privileged"`
	SeccompProfile        string   `toml:"seccompProfile"`
	SecurityLabelDisable  *bool    `toml:"securityLabelDisable"`
	SecurityLabelFileType string   `toml:"securityLabelFileType"`
	SecurityLabelLevel    string   `toml:"securityLabelLevel"`
	SecurityLabelNested   *bool    `toml:"securityLabelNested"`
	SecurityLabelType     string   `toml:"securityLabelType"`
	SecurityOpt           []string `toml:"securityOpt"`
	UserNS                string   `toml:"userns"`
}

type ResourcesConfig struct {
	Memory     string   `toml:"memory"`
	MemorySwap string   `toml:"memorySwap"`
	CPUs       string   `toml:"cpus"`
	CPUQuota   string   `toml:"cpuQuota"`
	PidsLimit  int      `toml:"pidsLimit"`
	Ulimits    []string `toml:"ulimits"`
}

type SecretConfig struct {
	Name    string `toml:"name"`
	Target  string `toml:"target"`
	Type    string `toml:"type"`
	UID     string `toml:"uid"`
	GID     string `toml:"gid"`
	Mode    string `toml:"mode"`
	Options string `toml:"options"`
}

type WorkspaceConfig struct {
	Mode string `toml:"mode"`
	// Source is the host directory to copy into the container workspace.
	Source string `toml:"source"`
	Target string `toml:"target"`
	Review string `toml:"review"`
	// Promote controls what happens after the diff is shown.
	// "off" (default): show diff and exit.
	// "prompt": ask the user whether to apply changes back to Source.
	// "auto": always apply changes back to Source without prompting.
	Promote string `toml:"promote"`
	// ExcludePatterns overrides the default workspace skip list when non-empty.
	// Default skips: .git .jj .go .direnv result node_modules
	// Set to an explicit list to control which directories are skipped.
	// Omitting "node_modules" includes it in the workspace copy.
	ExcludePatterns []string `toml:"excludePatterns"`
}

// AttachConfig controls how `graft attach` connects to a running container.
type AttachConfig struct {
	// TmuxSession is the tmux session name to attach to or create (default: "main").
	TmuxSession string `toml:"tmuxSession"`
	// Shell is the fallback interactive shell when tmux is unavailable (default: "sh").
	Shell string `toml:"shell"`
	// StartDelay is how long to wait after `graft up` before attaching.
	// Accepts Go duration strings: "500ms", "2s", "1m". Default: "500ms".
	StartDelay string `toml:"startDelay"`
}

// ShadowMount adds an extra writable path inside the container backed by a
// per-session copy of the host directory. Changes can be reviewed with
// `graft diff` and promoted back to the host with `graft promote`.
type ShadowMount struct {
	// Container is the path inside the container (e.g. "/workspace").
	Container string `toml:"container"`
	// Host is the host directory to seed the session from and promote changes
	// back to. Supports ~ expansion. May be empty for a blank writable dir.
	Host string `toml:"host"`
}

type HomeConfig struct {
	// Ephemeral is the legacy field; equivalent to Mode = "ephemeral".
	Ephemeral bool `toml:"ephemeral"`
	// Mode controls session persistence:
	//   "ephemeral"  — temp dir, wiped after each run.
	//   "persistent" — host dir survives across runs.
	//   "session"    — host dir is copied to a temp dir for each run; changes
	//                  are reviewed and optionally promoted back at session end.
	Mode string `toml:"mode"`
	// Source is the host path for Mode = "persistent" or "session". Supports ~ expansion.
	Source string `toml:"source"`
	Target string `toml:"target"`
	// Review shows a diff of home changes at session end (session mode only).
	// Set to "diff" to print a recursive diff before the promote step.
	Review string `toml:"review"`
	// Promote controls what happens to session changes at session end (session mode only).
	//   "auto"   — always apply changes back to Source.
	//   "prompt" — ask the user whether to apply changes.
	//   "never"  — always discard (default for session mode).
	Promote string `toml:"promote"`
	// Shadow defines additional writable paths inside the container. Each entry
	// is backed by a per-session copy of the host directory; use `graft diff`
	// to review changes and `graft promote` to copy them back to the host.
	Shadow []ShadowMount `toml:"shadow"`
}

// ProxyConfig is used in two roles:
//
//  1. On a container that IS a proxy (name = "proxy" TOML):
//     Listen, LogLevel, and Upstreams define the egress gateway behaviour.
//     The container's command should be ["graft", "proxy", "serve"].
//
//  2. On a container that USES a proxy:
//     Service names the proxy container in configRoot. graft ensures it is
//     running before starting this container and injects HTTP_PROXY /
//     HTTPS_PROXY automatically.
type ProxyConfig struct {
	// --- consumer side ---
	Service string `toml:"service"` // name of the proxy container to depend on
	Port    int    `toml:"port"`    // proxy port (default 8888)

	// --- server side ---
	Listen    int              `toml:"listen"`    // port to listen on (default 8888)
	LogLevel  string           `toml:"logLevel"`  // debug, info, warn, error
	Upstreams []UpstreamConfig `toml:"upstreams"` // ordered allow/deny rules
}

// UpstreamConfig describes one egress rule in the proxy allow-list.
type UpstreamConfig struct {
	// Host is the upstream hostname. Use "*" for the catch-all default rule.
	Host string `toml:"host"`
	// Port matches the destination port. 0 matches any port.
	Port int `toml:"port"`
	// Allow: true = forward, false = deny and log.
	Allow bool `toml:"allow"`
	// Secret is the name of a Podman secret whose value is used as the token.
	// The token is injected into every forwarded request; the agent never sees it.
	Secret string `toml:"secret"`
	// TokenHeader is the HTTP header to inject the token into (e.g. "x-api-key").
	TokenHeader string `toml:"tokenHeader"`
	// TokenPrefix is prepended to the token value (e.g. "Bearer ").
	TokenPrefix string `toml:"tokenPrefix"`
}

type ServiceConfig struct {
	Type            string `toml:"type"`
	Restart         string `toml:"restart"`
	RestartSec      string `toml:"restartSec"`
	TimeoutStartSec string `toml:"timeoutStartSec"`
	TimeoutStopSec  string `toml:"timeoutStopSec"`
	RemainAfterExit *bool  `toml:"remainAfterExit"`
	// RestartIfChanged controls whether NixOS automatically restarts this
	// container when its unit file changes during nixos-rebuild switch.
	// Set to false to keep a long-running container alive across rebuilds.
	RestartIfChanged *bool `toml:"restartIfChanged"`
}

type QuadletConfig struct {
	Container map[string][]string `toml:"container"`
	Service   map[string][]string `toml:"service"`
	Install   map[string][]string `toml:"install"`
}
