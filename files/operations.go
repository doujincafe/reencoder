package files

import (
	"context"
	"crypto/sha256"
	"fmt"
	"io"
	"io/fs"
	"log"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/briandowns/spinner"
	"github.com/rosedblabs/rosedb/v2"
)

func (file *FileInfo) reencodeFile(ctx context.Context) error {
	select {
	case <-ctx.Done():
		return nil
	default:
		args := ctx.Value("flac").([]string)
		if args != nil {
			args = append(args, file.AbsPath)
			cmd := exec.Command("flac", args...)
			if err := cmd.Run(); err != nil {
				if !strings.Contains(err.Error(), "interrupt") {
					log.Println(err.Error())
					return err
				}
			}
		} else {
			cmd := exec.Command("flac", "-8f", "-j4", file.AbsPath)
			if err := cmd.Run(); err != nil {
				if !strings.Contains(err.Error(), "interrupt") {
					log.Println(err.Error())
					return err
				}
			}
		}

		file.Encoder = ctx.Value("encoder").(string)
		file.Process = false
		return nil
	}
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

func getInfoFromFile(path string) (*FileInfo, error) {
	var filedata FileInfo

	filedata.Process = true

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

func updateSpinner(spin *spinner.Spinner, counter int64) {
	line := fmt.Sprintf(" Indexing flacs...\t %v", counter)
	spin.Suffix = line
}

func IndexFlacs(ctx context.Context) error {
	counter := ctx.Value("counter").(*int64)
	database := ctx.Value("database").(*rosedb.DB)

	batch := database.NewBatch(rosedb.DefaultBatchOptions)

	spin := spinner.New(spinner.CharSets[9], 100*time.Millisecond)

	updateSpinner(spin, *counter)

	spin.Start()

	/* wg := new(errgroup.Group)
	wg.SetLimit(100) */

	if err := filepath.WalkDir(ctx.Value("path").(string), func(path string, info fs.DirEntry, err error) error {
		select {
		case <-ctx.Done():
			spin.FinalMSG = "Stopping...\n"
			spin.Stop()
			return filepath.SkipAll
		default:
			if !info.IsDir() {
				if filepath.Ext(path) == ".flac" {
					/* wg.Go(func() error {
						select {
						case <-ctx.Done():
							return nil
						default:
							data, err := getInfoFromFile(path)
							if err != nil {
								return err
							}

							hashsum, err := getSha256(path)
							if err != nil {
								return err
							}
							updateSpinner(spin, *counter)
							return data.IndexFile(ctx, hashsum, batch)
						}
					}) */

					data, err := getInfoFromFile(path)
					if err != nil {
						return err
					}

					hashsum, err := getSha256(path)
					if err != nil {
						return err
					}
					updateSpinner(spin, *counter)
					return data.IndexFile(ctx, hashsum, batch)
				}

			}
			return nil
		}

	}); err != nil {
		return err
	}

	/* wg.Wait() */

	if err := batch.Commit(); err != nil {
		return err
	}
	spin.FinalMSG = fmt.Sprintf("Done indexing flacs: \t%v to process\n", *counter)
	spin.Stop()

	return nil
}
