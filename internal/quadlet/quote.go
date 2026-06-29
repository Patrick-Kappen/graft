package quadlet

import (
	"sort"
	"strings"
)

func quoteExec(args []string) string {
	parts := make([]string, len(args))
	for i, arg := range args {
		parts[i] = SystemdQuote(arg)
	}
	return strings.Join(parts, " ")
}

func SystemdQuote(s string) string {
	if s == "" {
		return `""`
	}
	if strings.IndexFunc(s, func(r rune) bool {
		return r == ' ' || r == '\t' || r == '\n' || r == '"' || r == '\\' || r == ';' || r == '#'
	}) == -1 {
		return s
	}
	return `"` + strings.ReplaceAll(strings.ReplaceAll(s, `\`, `\\`), `"`, `\"`) + `"`
}

func sortedKeys(values map[string]string) []string {
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}
