package config

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/BurntSushi/toml"
)

// LoadResolved loads a config file and recursively resolves its parents,
// returning a fully merged File with no remaining parents/children.
//
// searchDirs is the ordered list of directories to look in when resolving
// parent names.  The directory of `path` itself is always prepended.
func LoadResolved(path string, searchDirs []string) (*File, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}
	return loadResolved(abs, prependDir(filepath.Dir(abs), searchDirs), map[string]struct{}{})
}

func loadResolved(absPath string, searchDirs []string, visited map[string]struct{}) (*File, error) {
	if _, cycle := visited[absPath]; cycle {
		return nil, fmt.Errorf("config cycle detected: %s is its own ancestor", absPath)
	}
	visited[absPath] = struct{}{}
	defer delete(visited, absPath)

	file, err := Load(absPath)
	if err != nil {
		return nil, err
	}

	// Compute the effective parent list.
	parents := EffectiveParents(nil, file.Parents)
	if len(parents) == 0 {
		// No parents — apply own packageOps and return.
		file.Config.Runtime.Packages = ApplyPackageOps(
			file.Config.Runtime.Packages,
			file.Config.Runtime.PackageOps,
		)
		file.Config.Runtime.PackageOps = PackageOpsConfig{}
		file.Parents = RelationSet{}
		file.Children = RelationSet{}
		return file, nil
	}

	// Resolve each parent and merge them left-to-right.
	// Later parents win over earlier ones; the child wins over all.
	var base *File
	for _, parentName := range parents {
		parentPath, err := findConfig(parentName, searchDirs)
		if err != nil {
			return nil, fmt.Errorf("parent %q of %s: %w", parentName, absPath, err)
		}
		parentAbs, err := filepath.Abs(parentPath)
		if err != nil {
			return nil, err
		}
		resolved, err := loadResolved(parentAbs, prependDir(filepath.Dir(parentAbs), searchDirs), visited)
		if err != nil {
			return nil, err
		}
		if base == nil {
			base = resolved
		} else {
			base = MergeFiles(base, resolved)
		}
	}

	// Merge the child over the accumulated parent base.
	out := MergeFiles(base, file)

	// Apply the accumulated packageOps.
	out.Config.Runtime.Packages = ApplyPackageOps(
		out.Config.Runtime.Packages,
		out.Config.Runtime.PackageOps,
	)
	out.Config.Runtime.PackageOps = PackageOpsConfig{}
	return out, nil
}

// findConfig looks for <name>.toml in each searchDir in order.
func findConfig(name string, searchDirs []string) (string, error) {
	for _, dir := range searchDirs {
		candidate := filepath.Join(dir, name+".toml")
		if _, err := os.Stat(candidate); err == nil {
			return candidate, nil
		}
	}
	return "", fmt.Errorf("config %q not found in search dirs: %v", name, searchDirs)
}

// prependDir returns searchDirs with dir prepended (deduplicating if already present).
func prependDir(dir string, searchDirs []string) []string {
	result := make([]string, 0, len(searchDirs)+1)
	result = append(result, dir)
	for _, d := range searchDirs {
		if d != dir {
			result = append(result, d)
		}
	}
	return result
}

func Load(path string) (*File, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	var file File
	metadata, err := toml.Decode(string(data), &file)
	if err != nil {
		return nil, err
	}
	if undecoded := metadata.Undecoded(); len(undecoded) > 0 {
		return nil, fmt.Errorf("unknown TOML field %s", undecoded[0].String())
	}
	if err := file.Validate(); err != nil {
		return nil, err
	}
	return &file, nil
}
