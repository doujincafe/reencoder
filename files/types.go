package files

import (
	"errors"
	"time"
)

var (
	UpToDate      = errors.New("file is up to date")
	MovedFile     = errors.New("file was moved")
	NeedsReencode = errors.New("file needs to be reencoded")
)

type FileInfo struct {
	AbsPath string    `json:"abspath"`
	Modtime time.Time `json:"modtime"`
	Encoder string    `json:"encoder"`
	Process bool      `json:"process"`
}
