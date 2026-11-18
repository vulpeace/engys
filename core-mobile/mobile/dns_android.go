//go:build android && cmfa

package mobile

import (
	"github.com/metacubex/mihomo/dns"
	"strings"
)

func UpdateDNS(servers string) {
	dns.UpdateSystemDNS(strings.Fields(servers))
	dns.FlushCacheWithDefaultResolver()
}
