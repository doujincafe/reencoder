package files

import (
	"context"
	"crypto/sha256"
	"fmt"
	"io"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/briandowns/spinner"
	"golang.org/x/sync/errgroup"
)

func (file *FileInfo) reencodeFile(ctx context.Context) error {
	/* cmd := exec.Command("flac", "-8", "-f", file.AbsPath)
	if err := cmd.Run(); err != nil {
		log.Printf("%s\n", err.Error())
		return err
	}
	file.Encoder = ctx.Value("encoder").(string) */
	fmt.Println(file)
	return nil
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

func getInfoFromFile(path string, info fs.DirEntry) (*FileInfo, error) {
	var filedata FileInfo

	filedata.Process = true

	tmp, err := info.Info()
	if err != nil {
		return nil, err
	}

	filedata.Modtime = tmp.ModTime()

	abspath, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}

	filedata.AbsPath = abspath

	encoder, err := getEncoderVer(path)
	if err != nil {
		return nil, err
	}

	filedata.Encoder = encoder

	return &filedata, nil
}

func getSha256(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		return nil, err
	}
	return hash.Sum(nil), nil
}

func IndexFlacs(ctx context.Context) error {
	spin := spinner.New(spinner.CharSets[9], 100*time.Millisecond)
	spin.Suffix = " Indexing flacs..."

	spin.Start()

	wg := new(errgroup.Group)
	wg.SetLimit(100)

	if err := filepath.WalkDir(ctx.Value("path").(string), func(path string, info fs.DirEntry, err error) error {
		select {
		case <-ctx.Done():
			spin.FinalMSG = "Stopping...\n"
			spin.Stop()
			return filepath.SkipAll
		default:
			if err != nil {
				return err
			}
			if !info.IsDir() {
				if filepath.Ext(path) == ".flac" {
					wg.Go(func() error {
						select {
						case <-ctx.Done():
							return nil
						default:
							data, err := getInfoFromFile(path, info)
							if err != nil {
								return err
							}

							hashsum, err := getSha256(path)
							if err != nil {
								return err
							}

							return data.IndexFile(ctx, hashsum)
						}
					})
				}

			}
			return nil
		}

	}); err != nil {
		return err
	}

	wg.Wait()
	spin.FinalMSG = "Done indexing flacs\n"
	spin.Stop()
	return nil
}
