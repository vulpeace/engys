// Package mobile exposes a deliberately small gomobile-compatible Android API.
package mobile

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/netip"
	"os"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"

	"github.com/metacubex/mihomo/component/dialer"
	"github.com/metacubex/mihomo/component/process"
	"github.com/metacubex/mihomo/config"
	C "github.com/metacubex/mihomo/constant"
	"github.com/metacubex/mihomo/hub"
	"github.com/metacubex/mihomo/hub/executor"
	"github.com/metacubex/mihomo/hub/route"
	"github.com/metacubex/mihomo/listener"
	LC "github.com/metacubex/mihomo/listener/config"
	"github.com/metacubex/mihomo/listener/sing_tun"
	"github.com/metacubex/mihomo/log"
	"github.com/metacubex/mihomo/tunnel"
	"github.com/metacubex/mihomo/tunnel/statistic"
	"go4.org/netipx"
)

type Host interface {
	Protect(fd int32) bool
	ResolveUid(protocol int32, source, destination string) int32
	ResolvePackage(uid int32) string
}
type hostRef struct{ host Host }

var active atomic.Pointer[hostRef]
var mu sync.Mutex
var tun *sing_tun.Listener
var logMu sync.Mutex
var logs = []log.Event{}
var displayedLogLevel atomic.Int32
var hooks sync.Once

func init() {
	displayedLogLevel.Store(int32(log.INFO))
	sub := log.Subscribe()
	go func() {
		for event := range sub {
			appendLogEvent(event)
		}
	}()
}

// Subscribe exposes every severity independently of mihomo's console threshold.
// Keep a separate atomic threshold so collection does not race with the core's
// non-atomic global log.Level during config application.
func appendLogEvent(event log.Event) {
	if int32(event.LogLevel) < displayedLogLevel.Load() {
		return
	}
	logMu.Lock()
	defer logMu.Unlock()
	logs = append(logs, event)
	if len(logs) > 1000 {
		logs = logs[len(logs)-1000:]
	}
}

// HeapAllocated reports live Go object allocation, not process RSS. The core
// shares its Android process with Tauri and Kotlin, so its RSS cannot be split.
func HeapAllocated() int64 {
	var stats runtime.MemStats
	runtime.ReadMemStats(&stats)
	return int64(stats.HeapAlloc)
}

func Version() string { return "Mihomo Meta " + C.Version + " android" }
func Logs() string {
	logMu.Lock()
	defer logMu.Unlock()
	visible := []string{}
	threshold := displayedLogLevel.Load()
	for _, event := range logs {
		if int32(event.LogLevel) >= threshold {
			visible = append(visible, fmt.Sprintf("level=%s msg=%q", event.Type(), event.Payload))
		}
	}
	data, _ := json.Marshal(visible)
	return string(data)
}

func installHooks() {
	hooks.Do(func() {
		dialer.DefaultSocketHook = func(_, _ string, conn syscall.RawConn) error {
			var refused bool
			err := conn.Control(func(fd uintptr) {
				if h := active.Load(); h != nil {
					refused = !h.host.Protect(int32(fd))
				}
			})
			if err != nil {
				return err
			}
			if refused {
				return errors.New("Android refused VPN socket protection")
			}
			return nil
		}
		process.DefaultPackageNameResolver = func(metadata *C.Metadata) (string, error) {
			h := active.Load()
			if h == nil || metadata.RawSrcAddr == nil || metadata.RawDstAddr == nil {
				return "", process.ErrInvalidNetwork
			}
			protocol := int32(syscall.IPPROTO_TCP)
			if strings.HasPrefix(metadata.RawSrcAddr.Network(), "udp") {
				protocol = syscall.IPPROTO_UDP
			}
			uid := h.host.ResolveUid(protocol, metadata.RawSrcAddr.String(), metadata.RawDstAddr.String())
			if uid < 0 {
				return "", process.ErrNotFound
			}
			metadata.Uid = uint32(uid)
			return h.host.ResolvePackage(uid), nil
		}
	})
}

// Options is serialized across JNI rather than exporting Go collection types.
type vpnOptions struct {
	Enable     bool     `json:"enable"`
	IPv6       bool     `json:"ipv6"`
	MTU        uint32   `json:"mtu"`
	Include    []string `json:"include"`
	Exclude    []string `json:"exclude"`
	AllowMode  bool     `json:"allowMode"`
	Routes     []string `json:"routes"`
	Controller string   `json:"controller"`
	Secret     string   `json:"secret"`
}

func rawConfig(yaml string) (*config.RawConfig, error) {
	raw, err := config.UnmarshalRawConfig([]byte(yaml))
	if err != nil {
		return nil, err
	}
	// Android owns TUN and routing. System time writes can kill an Android process.
	raw.NTP.WriteToSystem = false
	raw.RedirPort = 0
	raw.TProxyPort = 0
	raw.Interface = ""
	raw.RoutingMark = 0
	raw.ExternalControllerRoutingMark = 0
	raw.IPTables.Enable = false
	return raw, nil
}
func options(raw *config.RawConfig) (vpnOptions, error) {
	t := raw.Tun
	if len(t.RouteAddressSet) > 0 || len(t.RouteExcludeAddressSet) > 0 || len(t.IncludeUID) > 0 || len(t.ExcludeUID) > 0 || len(t.IncludeUIDRange) > 0 || len(t.ExcludeUIDRange) > 0 || len(t.IncludeAndroidUser) > 0 || len(t.IncludeInterface) > 0 || len(t.ExcludeInterface) > 0 || len(t.IncludeMACAddress) > 0 || len(t.ExcludeMACAddress) > 0 || len(t.ExcludeSrcPort) > 0 || len(t.ExcludeSrcPortRange) > 0 || len(t.ExcludeDstPort) > 0 || len(t.ExcludeDstPortRange) > 0 {
		return vpnOptions{}, errors.New("Android VPN supports package lists and CIDR routes; UID, user, interface, MAC, port and route-set capture filters are unsupported")
	}
	mtu := t.MTU
	if mtu == 0 {
		mtu = 9000
	}
	if mtu < 1280 || mtu > 65535 {
		return vpnOptions{}, errors.New("Android VPN MTU must be between 1280 and 65535")
	}
	o := vpnOptions{Enable: t.Enable, IPv6: raw.IPv6, MTU: mtu, AllowMode: len(t.IncludePackage) > 0, Include: []string{}, Exclude: t.ExcludePackage, Routes: []string{}, Controller: "http://" + raw.ExternalController, Secret: raw.Secret}
	denied := map[string]bool{}
	for _, p := range t.ExcludePackage {
		denied[p] = true
	}
	for _, p := range t.IncludePackage {
		if !denied[p] {
			o.Include = append(o.Include, p)
		}
	}
	var routes netipx.IPSetBuilder
	if len(t.RouteAddress) == 0 {
		routes.AddPrefix(netip.MustParsePrefix("0.0.0.0/0"))
		if raw.IPv6 {
			routes.AddPrefix(netip.MustParsePrefix("::/0"))
		}
	} else {
		for _, p := range t.RouteAddress {
			if p.Addr().Is4() || raw.IPv6 {
				routes.AddPrefix(p)
			}
		}
	}
	for _, p := range t.RouteExcludeAddress {
		routes.RemovePrefix(p)
	}
	set, err := routes.IPSet()
	if err != nil {
		return o, err
	}
	for _, p := range set.Prefixes() {
		o.Routes = append(o.Routes, p.String())
	}
	return o, nil
}
func Options(yaml string) (string, error) {
	raw, err := rawConfig(yaml)
	if err != nil {
		return "", err
	}
	o, err := options(raw)
	if err != nil {
		return "", err
	}
	b, err := json.Marshal(o)
	return string(b), err
}
func parse(yaml, home string) (*config.Config, error) {
	if err := os.MkdirAll(home, 0700); err != nil {
		return nil, err
	}
	C.SetHomeDir(home)
	raw, err := rawConfig(yaml)
	if err != nil {
		return nil, err
	}
	if _, err = options(raw); err != nil {
		return nil, err
	}
	raw.Tun.Enable = false
	raw.Tun.AutoRoute = false
	raw.Tun.AutoDetectInterface = false
	raw.Tun.AutoRedirect = false
	raw.Tun.FileDescriptor = 0
	return config.ParseRawConfig(raw)
}
func Validate(yaml, home string) error {
	mu.Lock()
	defer mu.Unlock()
	_, err := parse(yaml, home)
	return err
}

// PrepareProtection must run before Android establishes its routes.
func PrepareProtection(host Host) { installHooks(); active.Store(&hostRef{host}) }

// Start owns fd on every path. Serialize all mutations with Stop/Validate.
func Start(yaml, home string, fd int32) error {
	mu.Lock()
	defer mu.Unlock()
	if fd >= 0 {
		defer syscall.Close(int(fd))
	}
	cfg, err := parse(yaml, home)
	if err != nil {
		return err
	}
	raw, err := rawConfig(yaml)
	if err != nil {
		return err
	}
	o, err := options(raw)
	if err != nil {
		return err
	}
	if tun != nil {
		tun.Close()
		tun = nil
	}
	displayedLogLevel.Store(int32(cfg.General.LogLevel))
	controllerAddress = cfg.Controller.ExternalController
	hub.ApplyConfig(cfg)
	if !o.Enable {
		active.Store(nil)
		return nil
	}
	if fd < 0 {
		return errors.New("missing Android TUN descriptor")
	}
	duplicate, err := syscall.Dup(int(fd))
	if err != nil {
		return err
	}
	// sing-tun owns duplicate when tunNew runs; these options avoid pre-open validation paths.
	addresses := []netip.Prefix{netip.MustParsePrefix("198.18.0.1/30")}
	var v6 []netip.Prefix
	if o.IPv6 {
		v6 = []netip.Prefix{netip.MustParsePrefix("fdfe:dcba:9876::1/126")}
	}
	hijack := []string{"0.0.0.0:53", "[::]:53"}
	if len(raw.Tun.DNSHijack) == 0 {
		hijack = nil
	}
	tun, err = sing_tun.New(LC.Tun{Enable: true, Device: "Mihomo", Stack: raw.Tun.Stack,
		FileDescriptor: duplicate, MTU: o.MTU, Inet4Address: addresses, Inet6Address: v6,
		DNSHijack: hijack, AutoRoute: false, AutoDetectInterface: false}, tunnel.Tunnel)
	return err
}
func Stop() {
	mu.Lock()
	defer mu.Unlock()
	if tun != nil {
		tun.Close()
		tun = nil
	}
	executor.Shutdown()
	listener.ReCreateHTTP(0, tunnel.Tunnel)
	listener.ReCreateSocks(0, tunnel.Tunnel)
	listener.ReCreateMixed(0, tunnel.Tunnel)
	listener.ReCreateRedir(0, tunnel.Tunnel)
	listener.ReCreateTProxy(0, tunnel.Tunnel)
	listener.ReCreateShadowSocks("", tunnel.Tunnel)
	listener.ReCreateVmess("", tunnel.Tunnel)
	listener.ReCreateTuic(LC.TuicServer{}, tunnel.Tunnel)
	listener.PatchInboundListeners(map[string]C.InboundListener{}, tunnel.Tunnel, true)
	listener.PatchTunnel(nil, tunnel.Tunnel)
	tunnel.OnSuspend()
	route.ReCreateServer(&route.Config{})
	controllerAddress = ""
	ResetNetwork()
	active.Store(nil)
}
func ResetNetwork() {
	statistic.DefaultManager.Range(func(c statistic.Tracker) bool { _ = c.Close(); return true })
}
