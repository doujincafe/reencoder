package main

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"

	"github.com/urfave/cli/v2"
)

func getLocalStorage() string {
	switch runtime.GOOS {
	case "windows":
		return os.Getenv("APPDATA")
	case "linux":
		home, _ := os.UserHomeDir()
		return filepath.Join(home, ".local", "share")
	case "darwin":
		home, _ := os.UserHomeDir()
		return filepath.Join(home, "Library", "Application Support")
	default:
		return ""
	}
}

func getDb(cCtx *cli.Context) (context.Context, error) {
	if cCtx.Path("database") == "" {
		localFolder := getLocalStorage()
		if localFolder == "" {
			return context.WithValue(cCtx.Context, "database", ""), errors.New("failed to locate application data folder")
		}

		return context.WithValue(cCtx.Context, "database", filepath.Join(localFolder, "reencoder.db")), nil
	}
	if _, err := os.Stat(cCtx.Path("database")); err != nil {
		return context.WithValue(cCtx.Context, "database", ""), err
	}
	return context.WithValue(cCtx.Context, "database", cCtx.Path("database")), nil
}

func initCmd(cCtx *cli.Context) (context.Context, error) {
	if _, err := os.Stat(cCtx.Path("path")); err != nil {
		return nil, err
	}

	ctx, err := getDb(cCtx)
	if err != nil {
		return nil, err
	}
	return context.WithValue(ctx, "path", cCtx.Path("path")), nil
}
