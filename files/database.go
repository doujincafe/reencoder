package files

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/tidwall/buntdb"
)

func evaluateFile(database *buntdb.DB, hashsum string, filedata FileInfo) error {
	return database.View(func(tx *buntdb.Tx) error {
		data, err := tx.Get(hashsum)
		if err != nil {
			return err
		}

		var info FileInfo
		if err := json.Unmarshal([]byte(data), &info); err != nil {
			return err
		}
		if info == filedata {
			return UpToDate
		}
		switch info.Modtime == filedata.Modtime {
		case true:
			if info.AbsPath == filedata.AbsPath {
				if info.Process {
					return NeedsReencode
				}
				return UpToDate
			}
			if info.Process {
				return NeedsReencode
			}
			return MovedFile
		default:
			return nil
		}
	})
}

func getEncoderVer(path string) (string, error) {
	out, err := exec.Command("metaflac", "--show-vendor-tag", path).Output()
	if err != nil {
		return "", err
	}

	r := regexp.MustCompile("libFLAC \\d\\.\\d\\.\\d")
	encoder := r.FindString(string(out))
	switch encoder {
	case "":
		return "", nil
	default:
		return strings.Split(encoder, " ")[1], nil
	}
}

func updateFile(database *buntdb.DB, hashsum string, filedata FileInfo) error {
	return database.Update(func(tx *buntdb.Tx) error {
		data, err := json.Marshal(filedata)
		if err != nil {
			return err
		}
		if _, _, err := tx.Set(hashsum, string(data), nil); err != nil {
			return err
		}
		return nil
	})
}

func ProcessFile(ctx context.Context, path string, info fs.DirEntry) error {
	tmp, err := info.Info()
	if err != nil {
		return err
	}

	modtime := tmp.ModTime()

	database := ctx.Value("database").(*buntdb.DB)

	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()

	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		return err
	}
	hashsum := fmt.Sprintf("%x", hash.Sum(nil))

	abspath, err := filepath.Abs(path)
	if err != nil {
		return err
	}

	encoder, err := getEncoderVer(path)

	filedata := FileInfo{AbsPath: abspath, Modtime: modtime, Encoder: encoder, Process: true}

	err = evaluateFile(database, hashsum, filedata)
	switch err {
	case MovedFile, UpToDate:
		if filedata.Encoder == ctx.Value("encoder").(string) {
			return nil
		}
	case buntdb.ErrNotFound, NeedsReencode:

	default:
		return err
	}
	if err := updateFile(database, hashsum, filedata); err != nil {
		return err
	}
	return nil
}
