package files

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"os"
	"strings"

	"github.com/rosedblabs/rosedb/v2"
	progressbar "github.com/schollz/progressbar/v3"
	"golang.org/x/sync/errgroup"
)

func decodeDbInfo(value []byte) (*FileInfo, error) {
	var info FileInfo
	if err := json.Unmarshal(value, &info); err != nil {
		return nil, err
	}
	return &info, nil
}

func getInfoFromDb(batch *rosedb.Batch, hashsum []byte) (*FileInfo, error) {
	exist, err := batch.Exist(hashsum)
	if err != nil {
		return nil, err
	}

	if !exist {
		return nil, rosedb.ErrKeyNotFound
	}

	value, err := batch.Get(hashsum)
	if err != nil {
		return nil, err
	}

	info, err := decodeDbInfo(value)
	if err != nil {
		return nil, err
	}
	return info, err
}

func (filedata *FileInfo) evaluateFile(batch *rosedb.Batch, hashsum []byte, encoder string) error {
	info, err := getInfoFromDb(batch, hashsum)
	if err != nil {
		return err
	}

	if info.Process {
		return ReencodeNeeded
	}
	if filedata.Encoder != encoder {
		return ReencodeNeeded
	}
	if info.AbsPath != filedata.AbsPath {
		return FileMoved
	}
	return ReencodeNotNeeded
}

func (filedata *FileInfo) updateFile(batch *rosedb.Batch, hashsum []byte) error {
	encoded, err := json.Marshal(filedata)
	if err != nil {
		return err
	}
	return batch.Put(hashsum, encoded)
}

func (filedata *FileInfo) IndexFile(ctx context.Context, hashsum []byte, batch *rosedb.Batch) error {
	encoder := ctx.Value("encoder").(string)
	counter := ctx.Value("counter").(*int64)

	err := filedata.evaluateFile(batch, hashsum, encoder)

	switch err {
	case FileMoved, ReencodeNotNeeded:
		filedata.Process = false
	case ReencodeNeeded, rosedb.ErrKeyNotFound:
		*counter += 1
	default:
		return err
	}

	if err := filedata.updateFile(batch, hashsum); err != nil {
		return err
	}

	return nil
}

func ReencodeFlacs(ctx context.Context) error {
	db := ctx.Value("database").(*rosedb.DB)
	bar := progressbar.NewOptions64(
		*ctx.Value("counter").(*int64),
		progressbar.OptionSetDescription("Reencoding..."),
		progressbar.OptionShowCount(),
	)

	batch := db.NewBatch(rosedb.DefaultBatchOptions)

	wg := new(errgroup.Group)
	wg.SetLimit(4)

	iterOpts := rosedb.DefaultIteratorOptions
	iterOpts.ContinueOnError = true
	iter := db.NewIterator(iterOpts)

	for iter.Rewind(); iter.Valid(); iter.Next() {
		select {
		case <-ctx.Done():
			wg.Wait()
			if err := batch.Commit(); err != nil {
				return err
			}
			bar.Exit()
			return nil
		default:
			item := iter.Item()
			key := item.Key
			info, err := decodeDbInfo(item.Value)
			if err != nil {
				log.Println(err.Error())
			}

			if !strings.Contains(info.AbsPath, ctx.Value("path").(string)) {
				continue
			} else if _, err := os.Stat(info.AbsPath); errors.Is(err, os.ErrNotExist) {
				if err := batch.Delete(key); err != nil {
					log.Println(err.Error())
				}
			} else if err != nil {
				log.Println(err.Error())
			} else {
				if info.Process {
					wg.Go(func() error {
						select {
						case <-ctx.Done():
							return nil
						default:
							if err := info.reencodeFile(ctx); err != nil {
								return err
							}
							if err := batch.Delete(key); err != nil {
								return err
							}
							key, err = getSha256(info.AbsPath)
							if err != nil {
								return err
							}
							if err := info.updateFile(batch, key); err != nil {
								return err
							}

							bar.Add64(1)
							return nil
						}
					})
				}
			}
		}
	}

	wg.Wait()
	if err := batch.Commit(); err != nil {
		return err
	}
	bar.Close()
	return nil
}
