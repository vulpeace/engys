package mobile

import (
	"fmt"
	"go.yaml.in/yaml/v3"
	"net"
	"strconv"
	"testing"
)

func TestControllerSkipsOccupiedAndProxyPorts(t *testing.T) {
	occupied, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer occupied.Close()
	port := occupied.Addr().(*net.TCPAddr).Port
	source := fmt.Sprintf("external-controller: 127.0.0.1:%d\nmixed-port: %d\nsecret: test-secret\ncustom-setting: retained\n", port, port+1)
	prepared, err := ControllerConfig(source, 0)
	if err != nil {
		t.Fatal(err)
	}
	var root map[string]any
	if err = yaml.Unmarshal([]byte(prepared), &root); err != nil {
		t.Fatal(err)
	}
	host, text, err := net.SplitHostPort(root["external-controller"].(string))
	if err != nil {
		t.Fatal(err)
	}
	selected, err := strconv.Atoi(text)
	if err != nil || host != "127.0.0.1" || selected <= port+1 {
		t.Fatalf("unexpected selected address: %s", root["external-controller"])
	}
	if root["secret"] != "test-secret" || root["custom-setting"] != "retained" || root["mixed-port"] != port+1 {
		t.Fatal("controller fallback modified unrelated settings")
	}
	available, err := net.Listen("tcp4", net.JoinHostPort(host, text))
	if err != nil {
		t.Fatalf("selected port was not available: %v", err)
	}
	available.Close()
}
func TestControllerReloadReusesItsOwnListener(t *testing.T) {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	port := listener.Addr().(*net.TCPAddr).Port
	selected, err := availableControllerPort(port, listener.Addr().String(), nil)
	if err != nil || selected != port {
		t.Fatalf("own listener was treated as a collision: %d, %v", selected, err)
	}
}
func TestControllerExhaustionDoesNotWrap(t *testing.T) {
	selected, err := availableControllerPort(65535, "", map[int]bool{65535: true})
	if err == nil || selected != 0 {
		t.Fatalf("expected exhaustion, got %d, %v", selected, err)
	}
	for _, source := range []string{"external-controller: 127.0.0.1:0", "external-controller: 127.0.0.1:65536"} {
		if _, err := ControllerConfig(source, 0); err == nil {
			t.Fatal("invalid controller port accepted")
		}
	}
}
