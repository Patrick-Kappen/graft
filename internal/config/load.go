package config

import (
	"fmt"
	"os"

	"github.com/BurntSushi/toml"
)

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
