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
}

type RuntimeConfig struct {
	Mode     string   `toml:"mode"`
	Packages []string `toml:"packages"`
	Command  []string `toml:"command"`
}

type ContainerConfig struct {
	Name        string            `toml:"name"`
	WorkingDir  string            `toml:"workingDir"`
	User        string            `toml:"user"`
	Group       string            `toml:"group"`
	Environment map[string]string `toml:"environment"`
}

type FilesystemConfig struct {
	ReadOnly      *bool          `toml:"readOnly"`
	ReadOnlyTmpfs *bool          `toml:"readOnlyTmpfs"`
	Tmpfs         []string       `toml:"tmpfs"`
	Volumes       []VolumeConfig `toml:"volumes"`
}

type VolumeConfig struct {
	Source string `toml:"source"`
	Target string `toml:"target"`
	Mode   string `toml:"mode"`
}

type NetworkConfig struct {
	Mode    string   `toml:"mode"`
	Publish []string `toml:"publish"`
}

type SecurityConfig struct {
	DropCapabilities []string `toml:"dropCapabilities"`
	AddCapabilities  []string `toml:"addCapabilities"`
	NoNewPrivileges  *bool    `toml:"noNewPrivileges"`
	UserNS           string   `toml:"userns"`
}

type ResourcesConfig struct {
	Memory    string `toml:"memory"`
	PidsLimit int    `toml:"pidsLimit"`
}
