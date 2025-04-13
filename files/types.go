package files

import (
	"errors"
)

var (
	ReencodeNotNeeded = errors.New("file is up to date")
	FileMoved         = errors.New("file was moved")
	ReencodeNeeded    = errors.New("file needs to be reencoded")
)

type FileInfo struct {
	AbsPath string `json:"abspath"`
	Encoder string `json:"encoder"`
	Process bool   `json:"process"`
}
