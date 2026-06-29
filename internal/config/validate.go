package config

import (
	"fmt"
	"reflect"
	"unicode"
)

func (f *File) Validate() error {
	if f.Version != 1 {
		return fmt.Errorf("unsupported or missing version %d", f.Version)
	}
	if f.Validation.Level != "" && f.Validation.Level != "off" && f.Validation.Level != "warn" && f.Validation.Level != "strict" {
		return fmt.Errorf("unsupported validation level %q; expected off, warn, or strict", f.Validation.Level)
	}
	if f.Deploy.Target != "" && f.Deploy.Target != "system" && f.Deploy.Target != "user" {
		return fmt.Errorf("unsupported deploy target %q; expected system or user", f.Deploy.Target)
	}
	if f.Config.Runtime.Mode != "" && f.Config.Runtime.Mode != "rootfs-store" {
		return fmt.Errorf("unsupported runtime mode %q; expected rootfs-store", f.Config.Runtime.Mode)
	}
	if err := validateNoControlChars(reflect.ValueOf(*f), ""); err != nil {
		return err
	}
	if err := validateVolumes(f.Config.Filesystem.Volumes); err != nil {
		return err
	}
	if err := validateDevices(f.Config.Filesystem.Devices); err != nil {
		return err
	}
	if err := validateNetworks(f.Config.Networks); err != nil {
		return err
	}
	if err := validateVolumeUnits(f.Config.Volumes); err != nil {
		return err
	}
	if err := validateQuadletConflicts(f.Config); err != nil {
		return err
	}
	if err := validateSecrets(f.Config.Secrets); err != nil {
		return err
	}
	if err := validateWorkspace(f.Config.Workspace); err != nil {
		return err
	}
	return nil
}

func validateWorkspace(workspace WorkspaceConfig) error {
	switch workspace.Mode {
	case "", "none", "copy":
		return nil
	default:
		return fmt.Errorf("unsupported workspace mode %q; expected none or copy", workspace.Mode)
	}
}

func validateVolumes(volumes []VolumeConfig) error {
	seenTargets := map[string]struct{}{}
	for _, volume := range volumes {
		if volume.Source == "" && volume.Target == "" && volume.Mode == "" {
			continue
		}
		if volume.Source == "" || volume.Target == "" {
			return fmt.Errorf("filesystem volume must set both source and target")
		}
		if _, ok := seenTargets[volume.Target]; ok {
			return fmt.Errorf("duplicate filesystem volume target %q", volume.Target)
		}
		seenTargets[volume.Target] = struct{}{}
	}
	return nil
}

func validateDevices(devices []DeviceConfig) error {
	for _, device := range devices {
		if device.Source == "" && device.Target == "" && device.Permissions == "" {
			continue
		}
		if device.Source == "" {
			return fmt.Errorf("filesystem device must set source")
		}
	}
	return nil
}

// validateUniqueNames checks that every item has a non-empty name and that no
// name is used twice. kind is used in error messages, e.g. "network unit".
func validateUniqueNames[T any](items []T, kind string, name func(T) string) error {
	seen := map[string]struct{}{}
	for _, item := range items {
		n := name(item)
		if n == "" {
			return fmt.Errorf("%s must set name", kind)
		}
		if _, ok := seen[n]; ok {
			return fmt.Errorf("duplicate %s name %q", kind, n)
		}
		seen[n] = struct{}{}
	}
	return nil
}

func validateNetworks(networks []NetworkUnitConfig) error {
	return validateUniqueNames(networks, "network unit", func(n NetworkUnitConfig) string { return n.Name })
}

func validateVolumeUnits(volumes []VolumeUnitConfig) error {
	return validateUniqueNames(volumes, "volume unit", func(v VolumeUnitConfig) string { return v.Name })
}

func validateQuadletConflicts(cfg Config) error {
	containerKeys := map[string]struct{}{
		"Rootfs": {}, "ContainerName": {}, "Exec": {}, "Network": {}, "Volume": {},
	}
	for key := range cfg.Quadlet.Container {
		if _, conflict := containerKeys[key]; conflict {
			return fmt.Errorf("config.quadlet.container.%s conflicts with typed graft renderer fields", key)
		}
	}
	return nil
}

func validateSecrets(secrets []SecretConfig) error {
	return validateUniqueNames(secrets, "secret", func(s SecretConfig) string { return s.Name })
}

func validateNoControlChars(value reflect.Value, path string) error {
	if !value.IsValid() {
		return nil
	}
	if value.Kind() == reflect.Pointer || value.Kind() == reflect.Interface {
		if value.IsNil() {
			return nil
		}
		return validateNoControlChars(value.Elem(), path)
	}
	switch value.Kind() {
	case reflect.String:
		for _, r := range value.String() {
			if unicode.IsControl(r) {
				return fmt.Errorf("control character is not allowed in TOML string field %s", path)
			}
		}
	case reflect.Struct:
		typeOfValue := value.Type()
		for i := 0; i < value.NumField(); i++ {
			field := typeOfValue.Field(i)
			fieldPath := field.Name
			if path != "" {
				fieldPath = path + "." + field.Name
			}
			if err := validateNoControlChars(value.Field(i), fieldPath); err != nil {
				return err
			}
		}
	case reflect.Slice, reflect.Array:
		for i := 0; i < value.Len(); i++ {
			if err := validateNoControlChars(value.Index(i), path); err != nil {
				return err
			}
		}
	case reflect.Map:
		iter := value.MapRange()
		for iter.Next() {
			if err := validateNoControlChars(iter.Key(), path); err != nil {
				return err
			}
			if err := validateNoControlChars(iter.Value(), path); err != nil {
				return err
			}
		}
	}
	return nil
}

func (f *File) IsNoop() bool {
	return isEmptyValue(reflect.ValueOf(f.Config))
}

func isEmptyValue(value reflect.Value) bool {
	if !value.IsValid() {
		return true
	}
	if value.Kind() == reflect.Pointer || value.Kind() == reflect.Interface {
		return value.IsNil()
	}
	switch value.Kind() {
	case reflect.String:
		return value.String() == ""
	case reflect.Bool:
		return !value.Bool()
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return value.Int() == 0
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64, reflect.Uintptr:
		return value.Uint() == 0
	case reflect.Float32, reflect.Float64:
		return value.Float() == 0
	case reflect.Slice, reflect.Array, reflect.Map:
		return value.Len() == 0
	case reflect.Struct:
		for i := 0; i < value.NumField(); i++ {
			if !isEmptyValue(value.Field(i)) {
				return false
			}
		}
		return true
	default:
		return value.IsZero()
	}
}
