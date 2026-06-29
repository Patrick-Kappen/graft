package config

type File struct {
	Version  int          `toml:"version"`
	Name     string       `toml:"name"`
	Parents  RelationSet  `toml:"parents"`
	Children RelationSet  `toml:"children"`
	Deploy   DeployConfig `toml:"deploy"`
	Config   Config       `toml:"config"`
}

type RelationSet struct {
	Add    []string `toml:"add"`
	Remove []string `toml:"remove"`
	Set    []string `toml:"set"`
}

type DeployConfig struct {
	Enable    bool   `toml:"enable"`
	Target    string `toml:"target"`
	Autostart *bool  `toml:"autostart"`
}

type Config struct {
	Runtime    RuntimeConfig    `toml:"runtime"`
	Container  ContainerConfig  `toml:"container"`
	Filesystem FilesystemConfig `toml:"filesystem"`
	Network    NetworkConfig    `toml:"network"`
	Security   SecurityConfig   `toml:"security"`
	Resources  ResourcesConfig  `toml:"resources"`
	Secrets    []SecretConfig   `toml:"secrets"`
	Service    ServiceConfig    `toml:"service"`
	Quadlet    QuadletConfig    `toml:"quadlet"`
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
	Name        string            `toml:"name"`
	Hostname    string            `toml:"hostname"`
	Entrypoint  []string          `toml:"entrypoint"`
	StopSignal  string            `toml:"stopSignal"`
	WorkingDir  string            `toml:"workingDir"`
	User        string            `toml:"user"`
	Group       string            `toml:"group"`
	Environment map[string]string `toml:"environment"`
	PodmanArgs  []string          `toml:"podmanArgs"`
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
	Mode    string   `toml:"mode"`
	Publish []string `toml:"publish"`
	DNS     []string `toml:"dns"`
	AddHost []string `toml:"addHost"`
}

type SecurityConfig struct {
	DropCapabilities     []string `toml:"dropCapabilities"`
	AddCapabilities      []string `toml:"addCapabilities"`
	NoNewPrivileges      *bool    `toml:"noNewPrivileges"`
	Privileged           *bool    `toml:"privileged"`
	SeccompProfile       string   `toml:"seccompProfile"`
	SecurityLabelDisable *bool    `toml:"securityLabelDisable"`
	SecurityOpt          []string `toml:"securityOpt"`
	UserNS               string   `toml:"userns"`
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
