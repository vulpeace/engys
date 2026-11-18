package mobile

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"net"
	"strconv"
	"syscall"

	"github.com/metacubex/chi"
	"github.com/metacubex/http"
	"github.com/metacubex/mihomo/hub/route"
	"go.yaml.in/yaml/v3"
)

var controllerAddress string // protected by mu
var controllerIdentity = newControllerIdentity()

func newControllerIdentity() string {
	value := make([]byte, 16)
	if _, err := rand.Read(value); err != nil {
		panic(err)
	}
	return hex.EncodeToString(value)
}
func init() {
	// The endpoint is inside mihomo's authenticated API group. It distinguishes
	// two clients even when a subscription supplies the same controller secret.
	route.Register(func(router chi.Router) {
		router.Get("/mihomo-client/instance", func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Type", "text/plain")
			_, _ = w.Write([]byte(controllerIdentity))
		})
	})
}
func ControllerIdentity() string { return controllerIdentity }

// ControllerConfig picks the first available loopback port without changing the
// stored profile. minimum is used to retry if another process wins the bind race.
func ControllerConfig(source string, minimum int32) (string, error) {
	mu.Lock()
	defer mu.Unlock()
	var root map[string]any
	if err := yaml.Unmarshal([]byte(source), &root); err != nil {
		return "", err
	}
	if root == nil {
		root = map[string]any{}
	}
	address, _ := root["external-controller"].(string)
	if address == "" {
		address = "127.0.0.1:9097"
	}
	_, portText, err := net.SplitHostPort(address)
	if err != nil {
		return "", fmt.Errorf("invalid controller address: %w", err)
	}
	preferred, err := strconv.Atoi(portText)
	if err != nil || preferred < 1 || preferred > 65535 {
		return "", errors.New("controller port must be between 1 and 65535")
	}
	if minimum < 0 || minimum > 65535 {
		return "", errors.New("no available controller port remains")
	}
	if int(minimum) > preferred {
		preferred = int(minimum)
	}
	raw, err := rawConfig(source)
	if err != nil {
		return "", err
	}
	// Do not select a port the same configuration will need for a proxy listener.
	excluded := map[int]bool{}
	for _, port := range []int{raw.Port, raw.SocksPort, raw.MixedPort, raw.RedirPort, raw.TProxyPort} {
		if port > 0 {
			excluded[port] = true
		}
	}
	port, err := availableControllerPort(preferred, controllerAddress, excluded)
	if err != nil {
		return "", err
	}
	root["external-controller"] = net.JoinHostPort("127.0.0.1", strconv.Itoa(port))
	output, err := yaml.Marshal(root)
	return string(output), err
}
func availableControllerPort(preferred int, current string, excluded map[int]bool) (int, error) {
	for port := preferred; port <= 65535; port++ {
		if excluded[port] {
			continue
		}
		address := net.JoinHostPort("127.0.0.1", strconv.Itoa(port))
		if address == current {
			return port, nil
		} // Reuse this core's controller on reload.
		listener, err := net.Listen("tcp4", address)
		if err == nil {
			_ = listener.Close()
			return port, nil
		}
		if !errors.Is(err, syscall.EADDRINUSE) {
			return 0, fmt.Errorf("cannot bind controller: %w", err)
		}
	}
	return 0, fmt.Errorf("no available controller port between %d and 65535", preferred)
}
