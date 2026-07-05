package runtime

import (
	"os"
	"path/filepath"
)

func CreateMinimalRootfs(rootfs string) error {
	for _, dir := range []string{"tmp", "run", "etc"} {
		if err := os.MkdirAll(filepath.Join(rootfs, dir), 0o755); err != nil {
			return err
		}
	}
	if err := os.WriteFile(filepath.Join(rootfs, "etc", "passwd"), []byte("root:x:0:0:root:/root:/bin/sh\n"), 0o644); err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(rootfs, "etc", "group"), []byte("root:x:0:\n"), 0o644)
}
