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
	Mode      string   `toml:"mode"`
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

type HomeConfig struct {
	// Ephemeral is the legacy field; equivalent to Mode = "ephemeral".
	Ephemeral bool `toml:"ephemeral"`
	// Mode controls session persistence: "ephemeral" (default; temp dir wiped
	// after each run) or "persistent" (host dir survives across runs).
	Mode string `toml:"mode"`
	// Source is the host path for Mode = "persistent". Supports ~ expansion.
	Source string `toml:"source"`
	Target string `toml:"target"`
	// Session enables session tracking (meta.json sidecar) for managed containers.
	Session bool `toml:"session"`
	// Shadow defines host directories to shadow-mount per session.
	Shadow []ShadowMount `toml:"shadow"`
}

// ShadowMount describes a host directory that is copied into a per-session
// temporary directory and bind-mounted into the container read-write. After the
// session the copy can be reviewed with `graft diff` and promoted back to the
// original with `graft promote`.
type ShadowMount struct {
	// Source is the host directory to shadow (supports ~ expansion).
	Source string `toml:"source"`
	// Target is the mount point inside the container. Defaults to Source.
	Target string `toml:"target"`
}

type ServiceConfig struct {
	Type            string `toml:"type"`
	Restart         string `toml:"restart"`
	RestartSec      string `toml:"restartSec"`
	TimeoutStartSec string `toml:"timeoutStartSec"`
	TimeoutStopSec  string `toml:"timeoutStopSec"`
	RemainAfterExit *bool  `toml:"remainAfterExit"`
}

type QuadletConfig struct {
	Container map[string][]string `toml:"container"`
	Service   map[string][]string `toml:"service"`
	Install   map[string][]string `toml:"install"`
}
