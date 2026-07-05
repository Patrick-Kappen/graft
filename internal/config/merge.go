package config

// MergeFiles merges two config files: base is the parent, override is the
// child.  The child always wins over the parent for scalar fields; slices and
// maps are unioned (child values take precedence on conflicts).
//
// PackageOps from the override are collected but NOT applied here; call
// ApplyPackageOps on the result if you want the final effective package list.
//
// Parents / Children are cleared on the returned file: they have been resolved.
func MergeFiles(base, override *File) *File {
	out := *override // shallow copy so we don't mutate the caller's data

	// --- top-level scalar ---
	if out.Name == "" {
		out.Name = base.Name
	}

	// Deploy: override wins field-by-field.
	if !out.Deploy.Enable {
		out.Deploy.Enable = base.Deploy.Enable
	}
	if out.Deploy.Target == "" {
		out.Deploy.Target = base.Deploy.Target
	}

	// Validation
	if out.Validation.Level == "" {
		out.Validation.Level = base.Validation.Level
	}

	// --- config.runtime ---
	out.Config.Runtime = mergeRuntime(base.Config.Runtime, override.Config.Runtime)

	// --- config.container ---
	out.Config.Container = mergeContainer(base.Config.Container, override.Config.Container)

	// --- config.filesystem ---
	out.Config.Filesystem = mergeFilesystem(base.Config.Filesystem, override.Config.Filesystem)

	// --- config.proxy ---
	out.Config.Proxy = mergeProxy(base.Config.Proxy, override.Config.Proxy)

	// --- config.network ---
	out.Config.Network = mergeNetwork(base.Config.Network, override.Config.Network)

	// --- config.networks (extra network units) ---
	out.Config.Networks = mergeNetworkUnits(base.Config.Networks, override.Config.Networks)

	// --- config.volumes (extra volume units) ---
	out.Config.Volumes = mergeVolumeUnits(base.Config.Volumes, override.Config.Volumes)

	// --- config.security ---
	out.Config.Security = mergeSecurity(base.Config.Security, override.Config.Security)

	// --- config.resources ---
	out.Config.Resources = mergeResources(base.Config.Resources, override.Config.Resources)

	// --- config.secrets ---
	out.Config.Secrets = mergeSecrets(base.Config.Secrets, override.Config.Secrets)

	// --- config.workspace ---
	out.Config.Workspace = mergeWorkspace(base.Config.Workspace, override.Config.Workspace)

	// --- config.home ---
	out.Config.Home = mergeHome(base.Config.Home, override.Config.Home)

	// --- config.service ---
	out.Config.Service = mergeService(base.Config.Service, override.Config.Service)

	// --- config.quadlet ---
	out.Config.Quadlet = mergeQuadlet(base.Config.Quadlet, override.Config.Quadlet)

	// Parents and children have been resolved — clear them.
	out.Parents = RelationSet{}
	out.Children = RelationSet{}

	return &out
}

// ApplyPackageOps applies packageOps (add / remove / replace) to a package
// list and returns the resulting slice.  Duplicates are removed.
func ApplyPackageOps(packages []string, ops PackageOpsConfig) []string {
	set := make(map[string]struct{}, len(packages))
	order := make([]string, 0, len(packages))
	for _, p := range packages {
		if _, ok := set[p]; !ok {
			set[p] = struct{}{}
			order = append(order, p)
		}
	}

	// Replace before add/remove so that removals can target the new name.
	for _, r := range ops.Replace {
		if _, ok := set[r.Name]; ok {
			delete(set, r.Name)
			if _, ok2 := set[r.With]; !ok2 {
				set[r.With] = struct{}{}
			}
			// Rebuild order with replacement in-place.
			for i, p := range order {
				if p == r.Name {
					order[i] = r.With
					break
				}
			}
		}
	}

	// Remove.
	for _, p := range ops.Remove {
		delete(set, p)
	}

	// Add (preserve order, append new ones at the end).
	for _, p := range ops.Add {
		if _, ok := set[p]; !ok {
			set[p] = struct{}{}
			order = append(order, p)
		}
	}

	// Rebuild slice in insertion order, skipping removed items.
	result := order[:0:0]
	for _, p := range order {
		if _, ok := set[p]; ok {
			result = append(result, p)
		}
	}
	return result
}

// EffectiveParents computes the resolved parent list from a RelationSet:
//   - Set replaces everything if non-empty.
//   - Otherwise: start with current list, apply Add, then Remove.
func EffectiveParents(current []string, rel RelationSet) []string {
	var base []string
	if len(rel.Set) > 0 {
		base = rel.Set
	} else {
		base = append([]string(nil), current...)
		base = appendUnique(base, rel.Add)
	}
	return removeAll(base, rel.Remove)
}

// --------------------------------------------------------------------------
// field-level merge helpers
// --------------------------------------------------------------------------

func mergeRuntime(base, override RuntimeConfig) RuntimeConfig {
	out := override
	if out.Mode == "" {
		out.Mode = base.Mode
	}
	if len(out.Command) == 0 {
		out.Command = base.Command
	}
	// Packages: union, base first.
	out.Packages = unionStrings(base.Packages, override.Packages)
	// PackageOps are accumulated; the caller applies them via ApplyPackageOps.
	out.PackageOps = mergePackageOps(base.PackageOps, override.PackageOps)
	return out
}

func mergePackageOps(base, override PackageOpsConfig) PackageOpsConfig {
	return PackageOpsConfig{
		Add:     unionStrings(base.Add, override.Add),
		Remove:  unionStrings(base.Remove, override.Remove),
		Replace: append(append([]PackageReplaceConfig(nil), base.Replace...), override.Replace...),
	}
}

func mergeContainer(base, override ContainerConfig) ContainerConfig {
	out := override
	// Scalars: child wins if non-empty.
	if out.Name == "" {
		out.Name = base.Name
	}
	if out.Hostname == "" {
		out.Hostname = base.Hostname
	}
	if out.Pod == "" {
		out.Pod = base.Pod
	}
	if len(out.Entrypoint) == 0 {
		out.Entrypoint = base.Entrypoint
	}
	if out.StopSignal == "" {
		out.StopSignal = base.StopSignal
	}
	if out.StopTimeout == 0 {
		out.StopTimeout = base.StopTimeout
	}
	if out.WorkingDir == "" {
		out.WorkingDir = base.WorkingDir
	}
	if out.User == "" {
		out.User = base.User
	}
	if out.Group == "" {
		out.Group = base.Group
	}
	if out.Timezone == "" {
		out.Timezone = base.Timezone
	}
	if out.Notify == "" {
		out.Notify = base.Notify
	}
	if out.RunInit == nil {
		out.RunInit = base.RunInit
	}
	if out.IP == "" {
		out.IP = base.IP
	}
	if out.IP6 == "" {
		out.IP6 = base.IP6
	}
	if out.SubUIDMap == "" {
		out.SubUIDMap = base.SubUIDMap
	}
	if out.SubGIDMap == "" {
		out.SubGIDMap = base.SubGIDMap
	}
	if out.ShmSize == "" {
		out.ShmSize = base.ShmSize
	}
	if out.LogDriver == "" {
		out.LogDriver = base.LogDriver
	}
	// Maps: base as foundation, override wins on key conflict.
	out.Annotations = mergeMaps(base.Annotations, override.Annotations)
	out.Environment = mergeMaps(base.Environment, override.Environment)
	// Booleans.
	if out.EnvironmentHost == nil {
		out.EnvironmentHost = base.EnvironmentHost
	}
	// Lists: union.
	out.EnvironmentFile = unionStrings(base.EnvironmentFile, override.EnvironmentFile)
	out.PodmanArgs = unionStrings(base.PodmanArgs, override.PodmanArgs)
	out.GlobalArgs = unionStrings(base.GlobalArgs, override.GlobalArgs)
	out.NetworkAlias = unionStrings(base.NetworkAlias, override.NetworkAlias)
	out.ExposeHostPort = unionStrings(base.ExposeHostPort, override.ExposeHostPort)
	out.UIDMap = unionStrings(base.UIDMap, override.UIDMap)
	out.GIDMap = unionStrings(base.GIDMap, override.GIDMap)
	out.Mask = unionStrings(base.Mask, override.Mask)
	out.UnmaskPaths = unionStrings(base.UnmaskPaths, override.UnmaskPaths)
	out.Sysctl = unionStrings(base.Sysctl, override.Sysctl)
	// Health: child wins field-by-field.
	out.Health = mergeHealth(base.Health, override.Health)
	return out
}

func mergeHealth(base, override HealthConfig) HealthConfig {
	out := override
	if out.Cmd == "" {
		out.Cmd = base.Cmd
	}
	if out.Interval == "" {
		out.Interval = base.Interval
	}
	if out.Timeout == "" {
		out.Timeout = base.Timeout
	}
	if out.Retries == 0 {
		out.Retries = base.Retries
	}
	if out.StartPeriod == "" {
		out.StartPeriod = base.StartPeriod
	}
	if out.OnFailure == "" {
		out.OnFailure = base.OnFailure
	}
	if out.StartupCmd == "" {
		out.StartupCmd = base.StartupCmd
	}
	if out.StartupInterval == "" {
		out.StartupInterval = base.StartupInterval
	}
	if out.StartupRetries == 0 {
		out.StartupRetries = base.StartupRetries
	}
	if out.StartupSuccess == 0 {
		out.StartupSuccess = base.StartupSuccess
	}
	if out.StartupTimeout == "" {
		out.StartupTimeout = base.StartupTimeout
	}
	return out
}

func mergeFilesystem(base, override FilesystemConfig) FilesystemConfig {
	out := override
	if out.ReadOnly == nil {
		out.ReadOnly = base.ReadOnly
	}
	if out.ReadOnlyTmpfs == nil {
		out.ReadOnlyTmpfs = base.ReadOnlyTmpfs
	}
	out.Tmpfs = unionStrings(base.Tmpfs, override.Tmpfs)
	out.Mounts = unionStrings(base.Mounts, override.Mounts)
	// Volumes: base first; override wins on same target.
	out.Volumes = mergeVolumes(base.Volumes, override.Volumes)
	out.Devices = mergeDevices(base.Devices, override.Devices)
	return out
}

// mergeVolumes returns base volumes merged with override volumes.
// If both have a volume with the same target, the override entry wins.
func mergeVolumes(base, override []VolumeConfig) []VolumeConfig {
	overrideByTarget := make(map[string]VolumeConfig, len(override))
	for _, v := range override {
		overrideByTarget[v.Target] = v
	}
	result := make([]VolumeConfig, 0, len(base)+len(override))
	for _, v := range base {
		if ov, ok := overrideByTarget[v.Target]; ok {
			result = append(result, ov)
			delete(overrideByTarget, v.Target)
		} else {
			result = append(result, v)
		}
	}
	// Append override volumes that had no matching base target.
	for _, v := range override {
		if _, ok := overrideByTarget[v.Target]; ok {
			result = append(result, v)
		}
	}
	return result
}

func mergeDevices(base, override []DeviceConfig) []DeviceConfig {
	byTarget := make(map[string]struct{}, len(base))
	result := append([]DeviceConfig(nil), base...)
	for _, v := range base {
		byTarget[v.Target] = struct{}{}
	}
	for _, v := range override {
		if _, ok := byTarget[v.Target]; !ok {
			result = append(result, v)
		}
	}
	return result
}

func mergeNetwork(base, override NetworkConfig) NetworkConfig {
	out := override
	if out.Mode == "" {
		out.Mode = base.Mode
	}
	out.Modes = unionStrings(base.Modes, override.Modes)
	out.Publish = unionStrings(base.Publish, override.Publish)
	out.DNS = unionStrings(base.DNS, override.DNS)
	out.DNSOption = unionStrings(base.DNSOption, override.DNSOption)
	out.DNSSearch = unionStrings(base.DNSSearch, override.DNSSearch)
	out.AddHost = unionStrings(base.AddHost, override.AddHost)
	return out
}

func mergeProxy(base, override ProxyConfig) ProxyConfig {
	out := override
	if out.Service == "" {
		out.Service = base.Service
	}
	if out.Port == 0 {
		out.Port = base.Port
	}
	if out.Listen == 0 {
		out.Listen = base.Listen
	}
	if out.LogLevel == "" {
		out.LogLevel = base.LogLevel
	}
	// Upstreams: base first, override appended (order matters for rule matching).
	out.Upstreams = append(append([]UpstreamConfig(nil), base.Upstreams...), override.Upstreams...)
	return out
}

func mergeNetworkUnits(base, override []NetworkUnitConfig) []NetworkUnitConfig {
	return mergeByName(base, override, func(n NetworkUnitConfig) string { return n.Name })
}

func mergeVolumeUnits(base, override []VolumeUnitConfig) []VolumeUnitConfig {
	return mergeByName(base, override, func(v VolumeUnitConfig) string { return v.Name })
}

func mergeSecurity(base, override SecurityConfig) SecurityConfig {
	out := override
	out.DropCapabilities = unionStrings(base.DropCapabilities, override.DropCapabilities)
	out.AddCapabilities = unionStrings(base.AddCapabilities, override.AddCapabilities)
	if out.NoNewPrivileges == nil {
		out.NoNewPrivileges = base.NoNewPrivileges
	}
	if out.Privileged == nil {
		out.Privileged = base.Privileged
	}
	if out.SeccompProfile == "" {
		out.SeccompProfile = base.SeccompProfile
	}
	if out.SecurityLabelDisable == nil {
		out.SecurityLabelDisable = base.SecurityLabelDisable
	}
	if out.SecurityLabelFileType == "" {
		out.SecurityLabelFileType = base.SecurityLabelFileType
	}
	if out.SecurityLabelLevel == "" {
		out.SecurityLabelLevel = base.SecurityLabelLevel
	}
	if out.SecurityLabelNested == nil {
		out.SecurityLabelNested = base.SecurityLabelNested
	}
	if out.SecurityLabelType == "" {
		out.SecurityLabelType = base.SecurityLabelType
	}
	out.SecurityOpt = unionStrings(base.SecurityOpt, override.SecurityOpt)
	if out.UserNS == "" {
		out.UserNS = base.UserNS
	}
	return out
}

func mergeResources(base, override ResourcesConfig) ResourcesConfig {
	out := override
	if out.Memory == "" {
		out.Memory = base.Memory
	}
	if out.MemorySwap == "" {
		out.MemorySwap = base.MemorySwap
	}
	if out.CPUs == "" {
		out.CPUs = base.CPUs
	}
	if out.CPUQuota == "" {
		out.CPUQuota = base.CPUQuota
	}
	if out.PidsLimit == 0 {
		out.PidsLimit = base.PidsLimit
	}
	out.Ulimits = unionStrings(base.Ulimits, override.Ulimits)
	return out
}

func mergeSecrets(base, override []SecretConfig) []SecretConfig {
	return mergeByName(base, override, func(s SecretConfig) string { return s.Name })
}

func mergeWorkspace(base, override WorkspaceConfig) WorkspaceConfig {
	out := override
	if out.Mode == "" {
		out.Mode = base.Mode
	}
	if out.Source == "" {
		out.Source = base.Source
	}
	if out.Target == "" {
		out.Target = base.Target
	}
	if out.Review == "" {
		out.Review = base.Review
	}
	if out.Promote == "" {
		out.Promote = base.Promote
	}
	if len(out.ExcludePatterns) == 0 {
		out.ExcludePatterns = base.ExcludePatterns
	}
	return out
}

func mergeHome(base, override HomeConfig) HomeConfig {
	out := override
	if !out.Ephemeral {
		out.Ephemeral = base.Ephemeral
	}
	if out.Mode == "" {
		out.Mode = base.Mode
	}
	if out.Source == "" {
		out.Source = base.Source
	}
	if out.Target == "" {
		out.Target = base.Target
	}
	return out
}

func mergeService(base, override ServiceConfig) ServiceConfig {
	out := override
	if out.Type == "" {
		out.Type = base.Type
	}
	if out.Restart == "" {
		out.Restart = base.Restart
	}
	if out.RestartSec == "" {
		out.RestartSec = base.RestartSec
	}
	if out.TimeoutStartSec == "" {
		out.TimeoutStartSec = base.TimeoutStartSec
	}
	if out.TimeoutStopSec == "" {
		out.TimeoutStopSec = base.TimeoutStopSec
	}
	if out.RemainAfterExit == nil {
		out.RemainAfterExit = base.RemainAfterExit
	}
	return out
}

func mergeQuadlet(base, override QuadletConfig) QuadletConfig {
	return QuadletConfig{
		Container: mergeMapsSlices(base.Container, override.Container),
		Service:   mergeMapsSlices(base.Service, override.Service),
		Install:   mergeMapsSlices(base.Install, override.Install),
	}
}

// --------------------------------------------------------------------------
// generic helpers
// --------------------------------------------------------------------------

// mergeByName unions two slices, preserving base order. Override items whose
// name already appears in base are dropped (base wins on conflict).
func mergeByName[T any](base, override []T, name func(T) string) []T {
	seen := make(map[string]struct{}, len(base))
	result := append([]T(nil), base...)
	for _, item := range base {
		seen[name(item)] = struct{}{}
	}
	for _, item := range override {
		if _, ok := seen[name(item)]; !ok {
			result = append(result, item)
		}
	}
	return result
}

// unionStrings returns a deduplicated slice of base + override (base order first).
func unionStrings(base, override []string) []string {
	seen := make(map[string]struct{}, len(base)+len(override))
	result := make([]string, 0, len(base)+len(override))
	for _, s := range base {
		if _, ok := seen[s]; !ok {
			seen[s] = struct{}{}
			result = append(result, s)
		}
	}
	for _, s := range override {
		if _, ok := seen[s]; !ok {
			seen[s] = struct{}{}
			result = append(result, s)
		}
	}
	return result
}

// appendUnique appends elements from add that are not already in dst.
func appendUnique(dst, add []string) []string {
	seen := make(map[string]struct{}, len(dst))
	for _, s := range dst {
		seen[s] = struct{}{}
	}
	for _, s := range add {
		if _, ok := seen[s]; !ok {
			dst = append(dst, s)
			seen[s] = struct{}{}
		}
	}
	return dst
}

// removeAll removes all elements in remove from src.
func removeAll(src, remove []string) []string {
	rm := make(map[string]struct{}, len(remove))
	for _, s := range remove {
		rm[s] = struct{}{}
	}
	result := src[:0:0]
	for _, s := range src {
		if _, ok := rm[s]; !ok {
			result = append(result, s)
		}
	}
	return result
}

// mergeMaps merges two string→string maps; override wins on key conflict.
func mergeMaps(base, override map[string]string) map[string]string {
	if len(base) == 0 && len(override) == 0 {
		return nil
	}
	out := make(map[string]string, len(base)+len(override))
	for k, v := range base {
		out[k] = v
	}
	for k, v := range override {
		out[k] = v
	}
	return out
}

// mergeMapsSlices merges two map[string][]string; override wins on key conflict.
func mergeMapsSlices(base, override map[string][]string) map[string][]string {
	if len(base) == 0 && len(override) == 0 {
		return nil
	}
	out := make(map[string][]string, len(base)+len(override))
	for k, v := range base {
		out[k] = append([]string(nil), v...)
	}
	for k, v := range override {
		out[k] = append([]string(nil), v...)
	}
	return out
}
