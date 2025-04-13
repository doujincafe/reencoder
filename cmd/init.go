package main

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"

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
	if cCtx.Path("dbfile") == "" {
		localFolder := getLocalStorage()
		if localFolder == "" {
			return context.WithValue(cCtx.Context, "dbfile", ""), errors.New("failed to locate application data folder")
		}

		return context.WithValue(cCtx.Context, "dbfile", filepath.Join(localFolder, "reencoder")), nil
	}
	if _, err := os.Stat(cCtx.Path("dbfile")); err != nil {
		return context.WithValue(cCtx.Context, "dbfile", ""), err
	}
	return context.WithValue(cCtx.Context, "dbfile", cCtx.Path("dbfile")), nil
}

func checkTools() error {
	if _, err := exec.LookPath("flac"); err != nil {
		return errors.New("missing flac executable")
	}
	if _, err := exec.LookPath("metaflac"); err != nil {
		return errors.New("missing metaflac executable")
	}
	return nil
}

func initArgs(cCtx *cli.Context) (context.Context, error) {
	if err := checkTools(); err != nil {
		return nil, err
	}

	if _, err := os.Stat(cCtx.Path("path")); err != nil {
		return nil, err
	}

	ctx, err := getDb(cCtx)
	if err != nil {
		return nil, err
	}

	ctx = context.WithValue(ctx, "path", cCtx.Path("path"))

	encoder, err := exec.Command("flac", "-v").Output()
	if err != nil {
		return nil, err
	}

	ctx = context.WithValue(ctx, "encoder", strings.ReplaceAll(strings.Split(string(encoder), " ")[1], "\n", ""))

	defargs := []string{"-8f", "-j4"}

	if cCtx.Value("flac") == nil {
		return context.WithValue(ctx, "flac", defargs), nil
	}
	return context.WithValue(ctx, "flac", cCtx.StringSlice("flac")), nil
}
