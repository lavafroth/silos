package main

import (
	"fmt"
	"path/filepath"
	"strconv"
)

func main() {
	documentsDirectory := "/home/h/Documents/"
	resumeFilename := "resume.pdf"
	version := 3
	whereIsMyResume :=
		filepath.Base(
			documentsDirectory + "CV" + "_v" + strconv.Itoa(version) + "/" + resumeFilename)
	fmt.Println(whereIsMyResume)
}
