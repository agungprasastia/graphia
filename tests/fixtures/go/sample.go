package sample

import (
	"fmt"
	"math"
)

type Server struct {
	port int
}

type Service interface {
	Start() error
	Stop() error
}

type ConfigMap map[string]string

func NewServer(port int) *Server {
	fmt.Println("creating server")
	return &Server{port: port}
}

func (s *Server) Start() error {
	s.handleRequests()
	return nil
}

func (s *Server) handleRequests() {
	computeStats(s.port)
}

func computeStats(val int) float64 {
	return math.Sqrt(float64(val))
}
