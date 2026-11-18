package mobile

import (
	"encoding/json"
	"github.com/metacubex/mihomo/config"
	"github.com/metacubex/mihomo/log"
	"net/netip"
	"testing"
)

func decodeOptions(t *testing.T, yaml string) vpnOptions {
	t.Helper()
	value, err := Options(yaml)
	if err != nil {
		t.Fatal(err)
	}
	var got vpnOptions
	if err := json.Unmarshal([]byte(value), &got); err != nil {
		t.Fatal(err)
	}
	return got
}
func TestIncludeAndExcludeNormalizeWithoutBroadening(t *testing.T) {
	got := decodeOptions(t, "tun:\n  enable: true\n  include-package: [com.a, com.b]\n  exclude-package: [com.b]\n")
	if !got.AllowMode || len(got.Include) != 1 || got.Include[0] != "com.a" {
		t.Fatalf("unexpected allowlist: %+v", got)
	}
	got = decodeOptions(t, "tun: {include-package: [com.a], exclude-package: [com.a]}")
	if !got.AllowMode || len(got.Include) != 0 {
		t.Fatalf("empty selection became unrestricted: %+v", got)
	}
}
func TestCIDRExclusionsAndIPv6(t *testing.T) {
	got := decodeOptions(t, "ipv6: true\ntun: {route-address: [10.0.0.0/8, '::/0'], route-exclude-address: [10.1.0.0/16]}\n")
	excluded := netip.MustParseAddr("10.1.0.1")
	included := netip.MustParseAddr("10.2.0.1")
	found, v6 := false, false
	for _, text := range got.Routes {
		p := netip.MustParsePrefix(text)
		if p.Contains(excluded) {
			t.Fatal("excluded address was captured")
		}
		found = found || p.Contains(included)
		v6 = v6 || p.Addr().Is6()
	}
	if !found || !v6 {
		t.Fatalf("missing routes: %v", got.Routes)
	}
	got = decodeOptions(t, "tun: {route-address: [10.0.0.0/8], route-exclude-address: [10.0.0.0/8]}")
	if len(got.Routes) != 0 {
		t.Fatalf("excluded all routes became default route: %v", got.Routes)
	}
}
func TestAndroidRejectsUnsupportedCaptureAndInvalidMTU(t *testing.T) {
	for _, yaml := range []string{"tun: {include-uid: [1000]}", "tun: {mtu: 1000}", "tun: {route-address: [not-a-cidr]}"} {
		if _, err := Options(yaml); err == nil {
			t.Fatalf("accepted %s", yaml)
		}
	}
}
func TestAndroidDisablesPrivilegedDesktopOperations(t *testing.T) {
	raw, err := rawConfig("ntp: {enable: true, write-to-system: true}\nredir-port: 1234\ntproxy-port: 1235\n")
	if err != nil {
		t.Fatal(err)
	}
	if raw.NTP.WriteToSystem || raw.RedirPort != 0 || raw.TProxyPort != 0 {
		t.Fatal("privileged desktop behavior retained")
	}
	raw = config.DefaultRawConfig()
	raw.Tun.IncludePackage = []string{"com.missing"}
	got, err := options(raw)
	if err != nil || !got.AllowMode {
		t.Fatal("allow mode must survive missing package resolution")
	}
}

func TestLogPanelHonorsEffectiveLevel(t *testing.T) {
	logMu.Lock()
	previous := logs
	logs = nil
	logMu.Unlock()
	previousLevel := displayedLogLevel.Load()
	defer func() {
		displayedLogLevel.Store(previousLevel)
		logMu.Lock()
		logs = previous
		logMu.Unlock()
	}()
	displayedLogLevel.Store(int32(log.WARNING))
	for _, severity := range []log.LogLevel{log.DEBUG, log.INFO, log.WARNING, log.ERROR} {
		appendLogEvent(log.Event{LogLevel: severity, Payload: severity.String()})
	}
	var visible []string
	if err := json.Unmarshal([]byte(Logs()), &visible); err != nil {
		t.Fatal(err)
	}
	if len(visible) != 2 || visible[0] != `level=warning msg="warning"` || visible[1] != `level=error msg="error"` {
		t.Fatalf("warning threshold did not filter logs: %v", visible)
	}
	displayedLogLevel.Store(int32(log.SILENT))
	if Logs() != "[]" {
		t.Fatal("silent must hide existing messages too")
	}
	displayedLogLevel.Store(int32(log.DEBUG))
	appendLogEvent(log.Event{LogLevel: log.DEBUG, Payload: "visible after override"})
	displayedLogLevel.Store(int32(log.ERROR))
	if err := json.Unmarshal([]byte(Logs()), &visible); err != nil {
		t.Fatal(err)
	}
	if len(visible) != 1 || visible[0] != `level=error msg="error"` {
		t.Fatalf("old lower-severity entries survived a level change: %v", visible)
	}
}
