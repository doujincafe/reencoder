package main

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	badger "github.com/dgraph-io/badger/v4"
	"github.com/dgraph-io/badger/v4/options"
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

	return context.WithValue(ctx, "encoder", strings.ReplaceAll(strings.Split(string(encoder), " ")[1], "\n", "")), nil
}

func Options(path string) badger.Options {
	return badger.Options{
		Dir:      path,
		ValueDir: path,

		MemTableSize:        64 << 20,
		BaseTableSize:       2 << 20,
		BaseLevelSize:       10 << 20,
		TableSizeMultiplier: 2,
		LevelSizeMultiplier: 10,
		MaxLevels:           7,
		NumGoroutines:       8,
		MetricsEnabled:      false,

		NumCompactors:           4, // Run at least 2 compactors. Zero-th compactor prioritizes L0.
		NumLevelZeroTables:      5,
		NumLevelZeroTablesStall: 15,
		NumMemtables:            5,
		BloomFalsePositive:      0.01,
		BlockSize:               4 * 1024,
		SyncWrites:              false,
		NumVersionsToKeep:       1,
		CompactL0OnClose:        true,
		VerifyValueChecksum:     false,
		Compression:             options.Snappy,
		BlockCacheSize:          256 << 20,
		IndexCacheSize:          0,

		// The following benchmarks were done on a 4 KB block size (default block size). The
		// compression is ratio supposed to increase with increasing compression level but since the
		// input for compression algorithm is small (4 KB), we don't get significant benefit at
		// level 3.
		// NOTE: The benchmarks are with DataDog ZSTD that requires CGO. Hence, no longer valid.
		// no_compression-16              10	 502848865 ns/op	 165.46 MB/s	-
		// zstd_compression/level_1-16     7	 739037966 ns/op	 112.58 MB/s	2.93
		// zstd_compression/level_3-16     7	 756950250 ns/op	 109.91 MB/s	2.72
		// zstd_compression/level_15-16    1	11135686219 ns/op	   7.47 MB/s	4.38
		// Benchmark code can be found in table/builder_test.go file
		ZSTDCompressionLevel: 1,

		// (2^30 - 1)*2 when mmapping < 2^31 - 1, max int32.
		// -1 so 2*ValueLogFileSize won't overflow on 32-bit systems.
		ValueLogFileSize: 1<<30 - 1,

		ValueLogMaxEntries: 1000000,

		VLogPercentile: 0.0,
		ValueThreshold: (1 << 20),

		Logger:                        nil,
		EncryptionKey:                 []byte{},
		EncryptionKeyRotationDuration: 10 * 24 * time.Hour, // Default 10 days.
		DetectConflicts:               true,
		NamespaceOffset:               -1,
	}
}
