#!/usr/bin/env bash
# bot-cooker installer and control tool for a Linux server you own.
#
#   curl -fsSL https://raw.githubusercontent.com/koval01/bot-cooker/main/deploy/install.sh | sudo bash
#
# Run from a terminal, it walks you through the setup in English or Russian:
# checks the server, the hostname and its DNS, sets up Docker, starts the trap
# behind Caddy (automatic HTTPS) and explains how to watch, stop and remove
# it. With options (and --yes) it runs unattended, e.g. from cloud-init.
#
# After installation it lives in /opt/bot-cooker/install.sh and is called by
# the `bot-cooker-ctl` command: status, stats, logs, stop, start, restart,
# update, auto-update on|off, uninstall, guide.
#
# Run it only on servers you control: the trap answers anyone who connects.
set -euo pipefail

REPO_DEFAULT="koval01/bot-cooker"
DIR="/opt/bot-cooker"
CTL="/usr/local/bin/bot-cooker-ctl"
UNIT="bot-cooker-update"

# Settings: from options, else from a previous installation, else asked.
DOMAIN=""
MODE=""             # https | http
VERSION=""
REPO=""
IMAGE=""
SOURCE=""
PACK=""
AUTO_UPDATE=""      # 1 | 0, only ever 1 by explicit consent
UI_LANG=""
ASSUME_YES=0
INSTALL_DOCKER=1
PURGE=0
SKIP_DNS=0
COMMAND="install"

# ---------------------------------------------------------------------------
# Messages
# ---------------------------------------------------------------------------

msg() {
  local key="$1"
  if [ "${UI_LANG:-en}" = ru ]; then
    case "$key" in
      welcome) echo "Установка bot-cooker — защитной ловушки (тарпита) для сканеров.
Ловушка отвечает на автоматические сканеры медленно и правдоподобными
приманками, чтобы атаки на инфраструктуру обходились дороже. Она ничего не
сканирует, не подключается к чужим серверам и никому не вредит.

Ставьте её только на свой сервер и на отдельное имя хоста, которым никто не
пользуется." ;;
      continue_q) echo "Продолжить?" ;;
      aborted) echo "Установка отменена, на сервере ничего не изменено." ;;
      need_root) echo "Нужны права root. Запустите так: curl -fsSL <адрес> | sudo bash" ;;
      need_linux) echo "Установщик работает только на Linux. Для Windows см. документацию." ;;
      need_tty) echo "Нет терминала для вопросов. Укажите параметры явно, например:
  bash install.sh --yes --domain trap.example.com
или --http-only. Список: bash install.sh --help" ;;
      checking) echo "Проверяю сервер" ;;
      arch_bad) echo "Архитектура %s не поддерживается (нужны x86_64 или aarch64)." ;;
      mem_low) echo "Памяти всего %s МБ. Ловушке хватает 512 МБ, но меньше — рискованно." ;;
      disk_low) echo "Свободно всего %s МБ на диске. Нужен хотя бы 1 ГБ для Docker-образов." ;;
      port_busy) echo "Порт %s занят другой программой:" ;;
      port_help) echo "Ловушке нужны порты 80 и 443. Остановите эту программу (например:
  systemctl disable --now nginx  или  systemctl disable --now apache2)
или используйте отдельный сервер." ;;
      existing) echo "bot-cooker уже установлен на этом сервере (%s)." ;;
      existing_menu) echo "  1) обновить и при необходимости изменить настройки (ключ и персона сохранятся)
  2) удалить
  3) ничего не делать" ;;
      choose) echo "Ваш выбор" ;;
      mode_q) echo "Как ловушка будет доступна?
  1) по HTTPS на своём имени хоста с автоматическим сертификатом (рекомендуется)
  2) только HTTP на порту 80 (если HTTPS уже делает ваш прокси или CDN)" ;;
      domain_q) echo "Имя хоста для ловушки (например backend-node-1.example.org)" ;;
      domain_bad) echo "Это не похоже на имя хоста. Нужно что-то вроде trap.example.org: без http://, без пути, без пробелов." ;;
      domain_ip) echo "Нужно имя хоста, а не IP-адрес: сертификат выдаётся на имя." ;;
      ip_lookup) echo "Определяю публичный адрес сервера" ;;
      ip_external) echo "Сервер за NAT; спрашиваю публичный адрес у %s." ;;
      ip_unknown) echo "Не удалось определить публичный IPv4 сервера." ;;
      ip_ask) echo "Введите публичный IPv4 этого сервера (его показывает панель провайдера)" ;;
      ip_is) echo "Публичный адрес сервера: %s" ;;
      dns_pointed_q) echo "DNS-запись A для %s уже указывает на %s?" ;;
      dns_howto) echo "Что сделать у регистратора или DNS-провайдера:
  тип:  A
  имя:  %s
  адрес: %s
  TTL:  минимальный (например 300)
  Если DNS у Cloudflare или похожего CDN — выключите проксирование
  («оранжевое облако» → «серое», DNS only): ловушка должна быть доступна напрямую.
  Если для имени есть запись AAAA (IPv6), удалите её, если только она не
  указывает на IPv6 этого сервера.
Обычно запись начинает работать через 1–10 минут." ;;
      dns_wait) echo "Нажмите Enter, когда добавите запись, чтобы проверить снова (или введите s, чтобы пропустить проверку)" ;;
      dns_checking) echo "Проверяю DNS %s через публичный резолвер" ;;
      dns_ok) echo "DNS в порядке: %s → %s" ;;
      dns_none) echo "Для %s пока нет записи A." ;;
      dns_other) echo "%s указывает на %s, а сервер — %s." ;;
      dns_you_said) echo "Вы ответили, что запись уже настроена, но проверка показывает другое. Так бывает:
  – запись ещё не разошлась (подождите несколько минут);
  – запись добавлена в другой зоне или с опечаткой в имени;
  – в имени указан полный домен дважды (например trap.example.org.example.org)." ;;
      dns_proxied) echo "%s ведёт на адрес Cloudflare (%s): включено проксирование.
Выключите его для этой записи (DNS only, «серое облако»). Через прокси CDN
сканеры увидят CDN, а не ловушку, а медленные ответы будут обрываться." ;;
      dns_aaaa) echo "У %s есть запись AAAA (%s), но у сервера нет такого IPv6. Let's Encrypt
предпочитает IPv6, и выдача сертификата сорвётся. Удалите эту запись AAAA." ;;
      dns_skip_warn) echo "Проверка DNS пропущена. Если запись неверна, сертификат не будет выдан;
установщик подскажет, что исправить." ;;
      dns_options) echo "  1) проверить ещё раз
  2) установить всё равно (сертификат появится, когда DNS заработает)
  3) переключиться на режим только HTTP
  4) выйти" ;;
      fw_q) echo "В облачном файрволе провайдера (Security Group, Firewall) открыты входящие TCP 80 и 443?" ;;
      fw_help) echo "Откройте входящие TCP 80 и 443 в панели провайдера: без этого Let's Encrypt
не выдаст сертификат, а сканеры не дойдут до ловушки. Это делается в разделе
Firewall / Security Group / Security List вашего облака." ;;
      fw_local) echo "На сервере включён %s, порты 80 и 443 в нём закрыты. Открыть их?" ;;
      fw_opened) echo "Порты 80 и 443 открыты в %s." ;;
      au_q) echo "Включить автоматические обновления?
Раз в сутки сервер проверяет новые релизы в github.com/%s и ставит их сам,
сохраняя ключ и персону. Перед установкой проверяется контрольная сумма, при
ошибке — откат на прежнюю версию. Для проверки сервер обращается к github.com.
Можно включить или выключить позже: bot-cooker-ctl auto-update on|off" ;;
      version_latest) echo "Последний релиз: %s" ;;
      version_none) echo "Не удалось узнать последний релиз (нет релизов или нет доступа к GitHub); беру образ с тегом latest." ;;
      summary) echo "Итак:" ;;
      s_mode_https) echo "  режим:          HTTPS для %s (сертификат Let's Encrypt)" ;;
      s_mode_http) echo "  режим:          только HTTP на порту 80" ;;
      s_version) echo "  версия:         %s" ;;
      s_au_on) echo "  автообновления: включены" ;;
      s_au_off) echo "  автообновления: выключены" ;;
      s_dir) echo "  каталог:        %s" ;;
      install_q) echo "Устанавливать?" ;;
      docker_install) echo "Устанавливаю Docker из пакетов дистрибутива" ;;
      docker_missing) echo "Пакеты этого дистрибутива не дают Docker с compose. Установите Docker Engine по
инструкции https://docs.docker.com/engine/install/ и запустите установщик снова." ;;
      need_docker) echo "Нужен Docker с плагином compose." ;;
      pulling) echo "Скачиваю образ %s" ;;
      pull_failed) echo "Не удалось скачать образ %s. Проверьте доступ сервера к ghcr.io и что такой
релиз существует (https://github.com/%s/releases)." ;;
      building) echo "Собираю образ из %s (несколько минут)" ;;
      starting) echo "Запускаю" ;;
      waiting) echo "Жду, пока ловушка ответит (до 3 минут)" ;;
      waiting_cert) echo "Жду сертификат Let's Encrypt и ответ ловушки (до 3 минут)" ;;
      ok_running) echo "bot-cooker работает." ;;
      persona_is) echo "Персона этой ловушки: %s" ;;
      fail_generic) echo "Ловушка пока не отвечает. Логи: bot-cooker-ctl logs" ;;
      fail_fw) echo "Let's Encrypt не может достучаться до сервера: входящие порты 80/443 закрыты.
Откройте их в файрволе провайдера и выполните: bot-cooker-ctl restart" ;;
      fail_dns) echo "Let's Encrypt не находит DNS-запись для %s. Проверьте запись A и выполните:
bot-cooker-ctl restart" ;;
      fail_other_host) echo "%s указывает не на этот сервер: Let's Encrypt проверил другой адрес.
Исправьте запись A (и удалите лишнюю AAAA), затем: bot-cooker-ctl restart" ;;
      fail_rate) echo "Let's Encrypt временно ограничил выдачу сертификатов для этого имени.
Подождите (обычно до часа) — Caddy повторит сам." ;;
      fail_trap) echo "Контейнер ловушки не запускается. Последние строки лога:" ;;
      uninstall_q) echo "Удалить bot-cooker? Контейнеры будут остановлены и удалены." ;;
      purge_q) echo "Удалить также ключ, токен и сертификат? Тогда при новой установке это будет уже другая «компания»." ;;
      removed) echo "bot-cooker удалён." ;;
      removed_kept) echo "Контейнеры удалены. Ключ и сертификат остались в %s (полностью: bot-cooker-ctl uninstall --purge)." ;;
      not_installed) echo "bot-cooker не установлен." ;;
      stopped) echo "Ловушка остановлена. Запустить снова: bot-cooker-ctl start" ;;
      started) echo "Ловушка запущена." ;;
      au_on) echo "Автообновления включены: проверка раз в сутки." ;;
      au_off) echo "Автообновления выключены." ;;
      ctl_missing) echo "Не удалось сохранить копию установщика для bot-cooker-ctl. Скачайте install.sh и запустите его файлом: sudo bash install.sh" ;;
      au_no_scheduler) echo "На сервере нет ни systemd, ни cron, поэтому автообновления не включены. Обновляйте вручную: bot-cooker-ctl update" ;;
      up_to_date) echo "Установлена последняя версия (%s)." ;;
      update_found) echo "Доступна версия %s (установлена %s), обновляю." ;;
      update_check_failed) echo "Не удалось проверить обновления на GitHub." ;;
      update_bad_sum) echo "Контрольная сумма нового установщика не совпала; обновление отменено." ;;
      update_rollback) echo "Новая версия не заработала; возвращаю %s." ;;
      update_done) echo "Обновлено до %s." ;;
      os_is) echo "Система: %s" ;;
      plan_title) echo "Что будет сделано в системе:" ;;
      report_title) echo "Что сделано на этом сервере:" ;;
      ch_pkg) echo "  • установлены пакеты: %s" ;;
      ch_repo) echo "  • добавлен официальный репозиторий Docker: %s" ;;
      ch_compose) echo "  • установлен плагин Docker Compose %s (проверена SHA-256)" ;;
      ch_service) echo "  • служба %s включена и запущена" ;;
      ch_fw) echo "  • открыты TCP %s в %s (правила помечены bot-cooker)" ;;
      ch_swap) echo "  • создан файл подкачки %s на %s МБ (добавлен в /etc/fstab)" ;;
      ch_sysctl) echo "  • сетевые настройки под тысячи долгих соединений: %s" ;;
      ch_files) echo "  • файлы ловушки: %s" ;;
      ch_ctl) echo "  • команда управления: %s" ;;
      ch_timer) echo "  • ежедневная проверка обновлений: %s" ;;
      p_docker) echo "  • установить Docker и Docker Compose" ;;
      p_fw) echo "  • открыть TCP %s в %s" ;;
      p_swap) echo "  • создать файл подкачки на %s МБ (памяти %s МБ, подкачки нет)" ;;
      p_sysctl) echo "  • поднять сетевые лимиты ядра (очереди соединений, файлы)" ;;
      p_trap) echo "  • запустить ловушку в Docker, файлы в %s" ;;
      docker_repo) echo "В пакетах этого дистрибутива нет Docker; подключаю официальный репозиторий Docker" ;;
      compose_get) echo "Устанавливаю плагин Docker Compose с github.com/docker/compose" ;;
      compose_bad) echo "Контрольная сумма Docker Compose не совпала; установка прервана." ;;
      reverted) echo "  • отменено: %s" ;;
      docker_kept) echo "Docker оставлен в системе (им могут пользоваться другие программы)." ;;
      yes_no) echo "[д/н]" ;;
      yes_chars) echo "дДyY" ;;
      *) echo "$key" ;;
    esac
  else
    case "$key" in
      welcome) echo "Installing bot-cooker, a defensive tarpit for scanners.
The trap answers automated scanners slowly, with convincing decoys, to make
attacks on infrastructure more expensive. It scans nothing, connects to no
one else's servers and harms no one.

Install it only on a server you own, on a hostname of its own that nobody
else uses." ;;
      continue_q) echo "Continue?" ;;
      aborted) echo "Installation cancelled; nothing on this server was changed." ;;
      need_root) echo "Root is required. Run it as: curl -fsSL <url> | sudo bash" ;;
      need_linux) echo "This installer supports Linux only. See the documentation for Windows." ;;
      need_tty) echo "No terminal to ask questions on. Pass the settings explicitly, e.g.:
  bash install.sh --yes --domain trap.example.com
or --http-only. All options: bash install.sh --help" ;;
      checking) echo "Checking the server" ;;
      arch_bad) echo "Architecture %s is not supported (x86_64 or aarch64 needed)." ;;
      mem_low) echo "Only %s MB of memory. The trap is fine with 512 MB; less is risky." ;;
      disk_low) echo "Only %s MB of free disk. At least 1 GB is needed for the Docker images." ;;
      port_busy) echo "Port %s is used by another program:" ;;
      port_help) echo "The trap needs ports 80 and 443. Stop that program (for example
  systemctl disable --now nginx  or  systemctl disable --now apache2)
or use a dedicated server." ;;
      existing) echo "bot-cooker is already installed on this server (%s)." ;;
      existing_menu) echo "  1) update, and change settings if needed (key and persona are kept)
  2) remove it
  3) do nothing" ;;
      choose) echo "Your choice" ;;
      mode_q) echo "How will the trap be reached?
  1) over HTTPS on its own hostname, with an automatic certificate (recommended)
  2) plain HTTP on port 80 only (when your proxy or CDN already does HTTPS)" ;;
      domain_q) echo "Hostname for the trap (e.g. backend-node-1.example.org)" ;;
      domain_bad) echo "That does not look like a hostname. Use something like trap.example.org: no http://, no path, no spaces." ;;
      domain_ip) echo "A hostname is needed, not an IP address: certificates are issued for names." ;;
      ip_lookup) echo "Finding this server's public address" ;;
      ip_external) echo "The server is behind NAT; asking %s for its public address." ;;
      ip_unknown) echo "Could not determine the server's public IPv4 address." ;;
      ip_ask) echo "Enter this server's public IPv4 (your provider's panel shows it)" ;;
      ip_is) echo "The server's public address: %s" ;;
      dns_pointed_q) echo "Does the DNS A record of %s already point to %s?" ;;
      dns_howto) echo "At your registrar or DNS provider, add:
  type:   A
  name:   %s
  value:  %s
  TTL:    the lowest offered (e.g. 300)
  If your DNS is at Cloudflare or a similar CDN, turn proxying off for it
  (orange cloud -> grey, \"DNS only\"): the trap must be reached directly.
  If the name has an AAAA (IPv6) record, delete it unless it points to this
  server's own IPv6 address.
The record usually starts working within 1-10 minutes." ;;
      dns_wait) echo "Press Enter when the record is in place to check again (or type s to skip the check)" ;;
      dns_checking) echo "Checking the DNS of %s through a public resolver" ;;
      dns_ok) echo "DNS is fine: %s -> %s" ;;
      dns_none) echo "%s has no A record yet." ;;
      dns_other) echo "%s points to %s, but this server is %s." ;;
      dns_you_said) echo "You answered that the record is in place, but the check shows otherwise. This happens when:
  - the record has not propagated yet (wait a few minutes);
  - it was added in another zone, or with a typo in the name;
  - the full domain was entered twice (e.g. trap.example.org.example.org)." ;;
      dns_proxied) echo "%s resolves to a Cloudflare address (%s): proxying is on.
Turn it off for this record (\"DNS only\", grey cloud). Behind a CDN proxy,
scanners see the CDN instead of the trap, and slow answers get cut off." ;;
      dns_aaaa) echo "%s has an AAAA record (%s) but this server has no such IPv6 address. Let's Encrypt
prefers IPv6, so the certificate would fail. Delete that AAAA record." ;;
      dns_skip_warn) echo "DNS check skipped. If the record is wrong, no certificate will be issued;
the installer will tell you what to fix." ;;
      dns_options) echo "  1) check again
  2) install anyway (the certificate arrives once DNS works)
  3) switch to plain HTTP only
  4) quit" ;;
      fw_q) echo "Does your provider's cloud firewall (Security Group, Firewall) allow inbound TCP 80 and 443?" ;;
      fw_help) echo "Open inbound TCP 80 and 443 in your provider's panel: without them Let's Encrypt
cannot issue the certificate and scanners cannot reach the trap. Look for
Firewall / Security Group / Security List in your cloud console." ;;
      fw_local) echo "%s is active on this server and ports 80 and 443 are closed in it. Open them?" ;;
      fw_opened) echo "Ports 80 and 443 opened in %s." ;;
      au_q) echo "Enable automatic updates?
Once a day the server checks github.com/%s for new releases and installs
them by itself, keeping the key and persona. The new installer's checksum is
verified first, and a failed update rolls back. Checking contacts github.com.
You can change this later: bot-cooker-ctl auto-update on|off" ;;
      version_latest) echo "Latest release: %s" ;;
      version_none) echo "Could not find the latest release (none yet, or GitHub unreachable); using the image tagged latest." ;;
      summary) echo "Summary:" ;;
      s_mode_https) echo "  mode:          HTTPS for %s (Let's Encrypt certificate)" ;;
      s_mode_http) echo "  mode:          plain HTTP on port 80" ;;
      s_version) echo "  version:       %s" ;;
      s_au_on) echo "  auto-updates:  on" ;;
      s_au_off) echo "  auto-updates:  off" ;;
      s_dir) echo "  directory:     %s" ;;
      install_q) echo "Install now?" ;;
      docker_install) echo "Installing Docker from the distribution's packages" ;;
      docker_missing) echo "This distribution's packages do not provide Docker with compose. Install Docker
Engine from https://docs.docker.com/engine/install/ and run the installer again." ;;
      need_docker) echo "Docker with the compose plugin is required." ;;
      pulling) echo "Pulling image %s" ;;
      pull_failed) echo "Could not pull image %s. Check that the server can reach ghcr.io and that the
release exists (https://github.com/%s/releases)." ;;
      building) echo "Building the image from %s (a few minutes)" ;;
      starting) echo "Starting" ;;
      waiting) echo "Waiting for the trap to answer (up to 3 minutes)" ;;
      waiting_cert) echo "Waiting for the Let's Encrypt certificate and the trap (up to 3 minutes)" ;;
      ok_running) echo "bot-cooker is running." ;;
      persona_is) echo "This trap's persona: %s" ;;
      fail_generic) echo "The trap does not answer yet. Logs: bot-cooker-ctl logs" ;;
      fail_fw) echo "Let's Encrypt cannot reach the server: inbound ports 80/443 are closed.
Open them in your provider's firewall, then run: bot-cooker-ctl restart" ;;
      fail_dns) echo "Let's Encrypt finds no DNS record for %s. Check the A record, then run:
bot-cooker-ctl restart" ;;
      fail_other_host) echo "%s does not point to this server: Let's Encrypt reached another address.
Fix the A record (and delete any stray AAAA), then: bot-cooker-ctl restart" ;;
      fail_rate) echo "Let's Encrypt is rate-limiting certificates for this name for now.
Wait (usually up to an hour); Caddy retries by itself." ;;
      fail_trap) echo "The trap container does not start. Last log lines:" ;;
      uninstall_q) echo "Remove bot-cooker? Its containers will be stopped and deleted." ;;
      purge_q) echo "Also delete the key, token and certificate? A new installation would then be a different \"company\"." ;;
      removed) echo "bot-cooker removed." ;;
      removed_kept) echo "Containers removed. The key and certificate are kept in %s (everything: bot-cooker-ctl uninstall --purge)." ;;
      not_installed) echo "bot-cooker is not installed." ;;
      stopped) echo "The trap is stopped. Start it again: bot-cooker-ctl start" ;;
      started) echo "The trap is running." ;;
      au_on) echo "Automatic updates are on: checked once a day." ;;
      au_off) echo "Automatic updates are off." ;;
      ctl_missing) echo "Could not keep a copy of the installer for bot-cooker-ctl. Download install.sh and run it as a file: sudo bash install.sh" ;;
      au_no_scheduler) echo "This server has neither systemd nor cron, so automatic updates are not enabled. Update by hand: bot-cooker-ctl update" ;;
      up_to_date) echo "The latest version is installed (%s)." ;;
      update_found) echo "Version %s is available (installed: %s); updating." ;;
      update_check_failed) echo "Could not check GitHub for updates." ;;
      update_bad_sum) echo "The new installer's checksum does not match; update cancelled." ;;
      update_rollback) echo "The new version did not come up; going back to %s." ;;
      update_done) echo "Updated to %s." ;;
      os_is) echo "System: %s" ;;
      plan_title) echo "These changes will be made to the system:" ;;
      report_title) echo "What was done on this server:" ;;
      ch_pkg) echo "  * installed packages: %s" ;;
      ch_repo) echo "  * added Docker's official package repository: %s" ;;
      ch_compose) echo "  * installed the Docker Compose plugin %s (SHA-256 verified)" ;;
      ch_service) echo "  * enabled and started the %s service" ;;
      ch_fw) echo "  * opened TCP %s in %s (rules marked bot-cooker)" ;;
      ch_swap) echo "  * created a swap file %s of %s MB (listed in /etc/fstab)" ;;
      ch_sysctl) echo "  * network settings for thousands of long connections: %s" ;;
      ch_files) echo "  * the trap's files: %s" ;;
      ch_ctl) echo "  * control command: %s" ;;
      ch_timer) echo "  * daily update check: %s" ;;
      p_docker) echo "  * install Docker and Docker Compose" ;;
      p_fw) echo "  * open TCP %s in %s" ;;
      p_swap) echo "  * create a %s MB swap file (%s MB of memory, no swap)" ;;
      p_sysctl) echo "  * raise kernel network limits (connection queues, files)" ;;
      p_trap) echo "  * run the trap in Docker, files in %s" ;;
      docker_repo) echo "This distribution's packages have no Docker; adding Docker's official repository" ;;
      compose_get) echo "Installing the Docker Compose plugin from github.com/docker/compose" ;;
      compose_bad) echo "The Docker Compose download's checksum does not match; stopping." ;;
      reverted) echo "  * undone: %s" ;;
      docker_kept) echo "Docker stays installed (other programs may use it)." ;;
      yes_no) echo "[y/n]" ;;
      yes_chars) echo "yY" ;;
      *) echo "$key" ;;
    esac
  fi
}

# printf with a message as the format: say KEY ARGS...
# shellcheck disable=SC2059 # the messages are the formats
fmt() { local f; f="$(msg "$1")"; shift; printf -- "$f" "$@"; }
say() { printf '\033[1m==>\033[0m %s\n' "$(fmt "$@")"; }
info() { printf '%s\n' "$(fmt "$@")"; }
warn() { printf '\033[33m!\033[0m %s\n' "$(fmt "$@")" >&2; }
die() { printf '\033[31mx\033[0m %s\n' "$(fmt "$@")" >&2; exit 1; }
log() { printf '%s %s\n' "$(date -u +%FT%TZ)" "$(fmt "$@")" >> "$DIR/update.log" 2>/dev/null || true; fmt "$@"; echo; }

# --- prompting ---------------------------------------------------------------

TTY=0
if [ -t 1 ] && { : </dev/tty; } 2>/dev/null; then TTY=1; fi
interactive() { [ "$TTY" = 1 ] && [ "$ASSUME_YES" = 0 ]; }

ask() { # ask PROMPT_TEXT DEFAULT -> answer on stdout
  local answer
  printf '%s%s: ' "$1" "${2:+ [$2]}" >/dev/tty
  IFS= read -r answer </dev/tty || answer=""
  printf '%s' "${answer:-$2}"
}

confirm() { # confirm TEXT DEFAULT(y|n)
  local answer chars
  chars="$(msg yes_chars)"
  printf '%s %s ' "$1" "$(msg yes_no)" >/dev/tty
  IFS= read -r answer </dev/tty || answer=""
  [ -z "$answer" ] && answer="$2"
  case "$answer" in [$chars]*) return 0 ;; *) return 1 ;; esac
}

choice() { # choice TEXT MAX DEFAULT -> a number 1..MAX
  local answer
  printf '%s\n' "$1" >/dev/tty
  while :; do
    answer="$(ask "$(msg choose)" "$3")"
    case "$answer" in *[!0-9]*|'') ;; *) [ "$answer" -ge 1 ] && [ "$answer" -le "$2" ] && { echo "$answer"; return; } ;; esac
  done
}

# ---------------------------------------------------------------------------
# Options
# ---------------------------------------------------------------------------

usage() {
  cat <<'EOF'
Usage:
  install.sh [options]              install or update (asks questions in a terminal)
  install.sh status|stats|logs|stop|start|restart|update|guide
  install.sh auto-update on|off
  install.sh uninstall [--purge]

Install options:
  --domain NAME        hostname whose A record points at this server (HTTPS mode)
  --http-only          plain HTTP on port 80, no certificate
  --lang en|ru         language of messages
  --yes                no questions: use options and previous answers
  --auto-update        enable automatic updates (explicit consent)
  --no-auto-update     disable them
  --version X.Y.Z      release to run (default: the latest)
  --repo OWNER/NAME    GitHub repository of releases and image (default: koval01/bot-cooker)
  --image REF          image repository (default: ghcr.io/<repo>)
  --source DIR         build the image from a source checkout instead of pulling
  --pack DIR           serve a content pack from DIR
  --skip-dns-check     do not verify the hostname's DNS
  --no-docker-install  fail instead of installing Docker when it is missing
EOF
}

case "${1:-}" in
  status|stats|logs|stop|start|restart|update|guide|auto-update|uninstall) COMMAND="$1"; shift ;;
esac

ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --domain) DOMAIN="${2:?--domain needs a value}"; MODE="https"; shift 2 ;;
    --http-only) MODE="http"; shift ;;
    --lang) UI_LANG="${2:?--lang needs en or ru}"; shift 2 ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    --auto-update) AUTO_UPDATE=1; shift ;;
    --no-auto-update) AUTO_UPDATE=0; shift ;;
    --version) VERSION="${2:?--version needs a value}"; VERSION="${VERSION#v}"; shift 2 ;;
    --repo) REPO="${2:?--repo needs owner/name}"; shift 2 ;;
    --image) IMAGE="${2:?--image needs a value}"; shift 2 ;;
    --source) SOURCE="${2:?--source needs a directory}"; shift 2 ;;
    --pack) PACK="${2:?--pack needs a directory}"; shift 2 ;;
    --skip-dns-check) SKIP_DNS=1; shift ;;
    --no-docker-install) INSTALL_DOCKER=0; shift ;;
    --purge) PURGE=1; shift ;;
    --auto) ASSUME_YES=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) ARGS+=("$1"); shift ;;
  esac
done

# Settings of a previous installation fill in what was not given.
CONF="$DIR/install.conf"
load_conf() {
  [ -f "$CONF" ] || return 0
  local key value
  while IFS='=' read -r key value; do
    value="${value#\'}"; value="${value%\'}"
    case "$key" in
      DOMAIN) [ -n "$DOMAIN" ] || DOMAIN="$value" ;;
      MODE) [ -n "$MODE" ] || MODE="$value" ;;
      REPO) [ -n "$REPO" ] || REPO="$value" ;;
      IMAGE) [ -n "$IMAGE" ] || IMAGE="$value" ;;
      SOURCE) [ -n "$SOURCE" ] || SOURCE="$value" ;;
      PACK) [ -n "$PACK" ] || PACK="$value" ;;
      AUTO_UPDATE) [ -n "$AUTO_UPDATE" ] || AUTO_UPDATE="$value" ;;
      UI_LANG) [ -n "$UI_LANG" ] || UI_LANG="$value" ;;
      VERSION) INSTALLED_VERSION="$value" ;;
    esac
  done < "$CONF"
}
INSTALLED_VERSION=""
load_conf
# An installation made before install.conf existed: its .env names the domain.
if [ -z "$DOMAIN" ] && [ -f "$DIR/.env" ]; then
  DOMAIN="$(sed -n 's/^DOMAIN=//p' "$DIR/.env" | tail -n 1 || true)"
  [ -n "$DOMAIN" ] && [ -z "$MODE" ] && MODE="https"
fi

if [ -z "$UI_LANG" ]; then
  case "${LC_ALL:-${LANG:-}}" in ru*) UI_LANG=ru ;; *) UI_LANG=en ;; esac
  if interactive && [ "$COMMAND" = install ]; then
    printf '%s\n' "Language / Язык:  1) English  2) Русский" >/dev/tty
    case "$(ask "1/2" "$([ "$UI_LANG" = ru ] && echo 2 || echo 1)")" in 2) UI_LANG=ru ;; *) UI_LANG=en ;; esac
  fi
fi
case "$UI_LANG" in en|ru) ;; *) UI_LANG=en ;; esac
REPO="${REPO:-$REPO_DEFAULT}"
IMAGE="${IMAGE:-ghcr.io/$REPO}"

[ "$(id -u)" -eq 0 ] || die need_root
[ "$(uname -s)" = Linux ] || die need_linux

compose() { docker compose --project-name bot-cooker --project-directory "$DIR" -f "$DIR/compose.yml" "$@"; }
installed() { [ -f "$DIR/compose.yml" ] && [ -f "$DIR/.env" ]; }
env_get() { sed -n "s/^$1=//p" "$DIR/.env" 2>/dev/null | tail -n 1; }

# ---------------------------------------------------------------------------
# Network facts
# ---------------------------------------------------------------------------

valid_v4() { printf '%s' "$1" | grep -Eq '^([0-9]{1,3}\.){3}[0-9]{1,3}$'; }

private_v4() {
  case "$1" in
    10.*|127.*|169.254.*|192.168.*|172.1[6-9].*|172.2[0-9].*|172.3[01].*) return 0 ;;
    100.6[4-9].*|100.[7-9][0-9].*|100.1[01][0-9].*|100.12[0-7].*) return 0 ;;
    *) return 1 ;;
  esac
}

# shellcheck disable=SC2086 # splitting the address on dots is the point
v4_int() { local IFS=.; set -- $1; echo $(( ($1 << 24) + ($2 << 16) + ($3 << 8) + $4 )); }

in_cidr() { # in_cidr IP NET/LEN
  local ip net len mask
  ip="$(v4_int "$1")"; net="$(v4_int "${2%/*}")"; len="${2#*/}"
  mask=$(( len == 0 ? 0 : (0xFFFFFFFF << (32 - len)) & 0xFFFFFFFF ))
  [ $(( ip & mask )) -eq $(( net & mask )) ]
}

# Cloudflare's published proxy ranges (https://www.cloudflare.com/ips-v4).
CLOUDFLARE_V4="173.245.48.0/20 103.21.244.0/22 103.22.200.0/22 103.31.4.0/22 141.101.64.0/18 108.162.192.0/18
190.93.240.0/20 188.114.96.0/20 197.234.240.0/22 198.41.128.0/17 162.158.0.0/15 104.16.0.0/13 104.24.0.0/14
172.64.0.0/13 131.0.72.0/22"
cloudflare_ip() { local n; for n in $CLOUDFLARE_V4; do in_cidr "$1" "$n" && return 0; done; return 1; }

# The address the internet sees. The routing table answers without any
# traffic when the server has a public address of its own; behind NAT (many
# clouds), ask a "what is my IP" service, and say which.
public_v4() {
  local ip url
  ip="$(ip -4 route get 1.1.1.1 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p' | head -n 1 || true)"
  if [ -n "$ip" ] && ! private_v4 "$ip"; then echo "$ip"; return; fi
  for url in https://api.ipify.org https://ifconfig.me/ip https://icanhazip.com; do
    info ip_external "${url#https://}" >&2
    ip="$(curl -fsS4 --max-time 6 "$url" 2>/dev/null | tr -d ' \r\n')" || ip=""
    if valid_v4 "$ip"; then echo "$ip"; return; fi
  done
}

# Empty when the server has no IPv6 route.
public_v6() { ip -6 route get 2606:4700:4700::1111 2>/dev/null | sed -n 's/.* src \([0-9a-f:]*\).*/\1/p' | head -n 1 || true; }

# Records as the internet sees them: a public resolver, not a local cache.
resolve() { # resolve A|AAAA NAME
  if command -v dig >/dev/null; then
    dig +short "$1" "$2" @1.1.1.1 2>/dev/null | grep -E '^[0-9a-fA-F:.]+$' | grep -v '\.$' || true
  elif [ "$1" = A ]; then
    getent ahostsv4 "$2" 2>/dev/null | awk '{print $1}' | sort -u || true
  else
    # glibc maps A records into ::ffff:a.b.c.d when there is no AAAA.
    getent ahostsv6 "$2" 2>/dev/null | awk '{print $1}' | grep ':' | grep -vi '^::ffff:' | sort -u || true
  fi
}

valid_domain() { printf '%s' "$1" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?(\.[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?)+$' && [ "${#1}" -le 253 ]; }

port_user() { ss -Hltnp "sport = :$1" 2>/dev/null | head -n 1 || true; }
port_busy() {
  local who; who="$(port_user "$1")"
  [ -n "$who" ] || return 1
  # Our own containers holding it is fine (re-install, update).
  ! docker ps --format '{{.Names}} {{.Ports}}' 2>/dev/null | grep -E '^bot-cooker-' | grep -q ":$1->"
}

# ---------------------------------------------------------------------------
# Steps of an installation
# ---------------------------------------------------------------------------

preflight() {
  say checking
  case "$(uname -m)" in x86_64|amd64|aarch64|arm64) ;; *) die arch_bad "$(uname -m)" ;; esac
  local mem disk port
  mem="$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo 2>/dev/null || echo 0)"
  [ "$mem" -ge 450 ] || warn mem_low "$mem"
  disk="$(df -Pm / 2>/dev/null | awk 'NR==2 {print $4}' || echo 99999)"
  [ "$disk" -ge 1024 ] || die disk_low "$disk"
  for port in 80 $([ "$MODE" = http ] || echo 443); do
    if port_busy "$port"; then
      warn port_busy "$port"
      port_user "$port" >&2
      die port_help
    fi
  done
}

choose_mode() {
  [ -n "$MODE" ] && return
  if interactive; then
    case "$(choice "$(msg mode_q)" 2 1)" in 2) MODE=http ;; *) MODE=https ;; esac
  else
    die need_tty
  fi
}

choose_domain() {
  [ "$MODE" = https ] || return 0
  while :; do
    if [ -z "$DOMAIN" ]; then
      interactive || die need_tty
      DOMAIN="$(ask "$(msg domain_q)" "")"
    fi
    DOMAIN="$(printf '%s' "$DOMAIN" | tr '[:upper:]' '[:lower:]' | sed -e 's#^https\?://##' -e 's#/.*$##' -e 's/\.$//')"
    if valid_v4 "$DOMAIN"; then warn domain_ip
    elif valid_domain "$DOMAIN"; then return 0
    else warn domain_bad
    fi
    interactive || exit 1
    DOMAIN=""
  done
}

# Make sure the hostname reaches this server before asking Let's Encrypt for
# a certificate: explain what to set, re-check, and catch a wrong "yes".
check_dns() {
  [ "$MODE" = https ] && [ "$SKIP_DNS" = 0 ] || return 0
  say ip_lookup
  local ip said_yes=0 a aaaa v6
  ip="$(public_v4)"
  if [ -z "$ip" ]; then
    warn ip_unknown
    interactive || return 0
    while :; do ip="$(ask "$(msg ip_ask)" "")"; valid_v4 "$ip" && break; done
  fi
  info ip_is "$ip"
  if interactive; then
    if confirm "$(fmt dns_pointed_q "$DOMAIN" "$ip")" y; then said_yes=1; else
      info dns_howto "$DOMAIN" "$ip"
      [ -z "$(ask "$(msg dns_wait)" "")" ] || { warn dns_skip_warn; return 0; }
    fi
  fi
  while :; do
    say dns_checking "$DOMAIN"
    a="$(resolve A "$DOMAIN" | tr '\n' ' ' | sed 's/ $//' || true)"
    aaaa="$(resolve AAAA "$DOMAIN" | tr '\n' ' ' | sed 's/ $//' || true)"
    v6="$(public_v6)"
    local problem=""
    if [ -z "$a" ]; then
      problem="none"; warn dns_none "$DOMAIN"
    elif cloudflare_ip "${a%% *}"; then
      problem="proxied"; warn dns_proxied "$DOMAIN" "$a"
    elif ! printf ' %s ' "$a" | grep -q " $ip "; then
      problem="other"; warn dns_other "$DOMAIN" "$a" "$ip"
    elif [ -n "$aaaa" ] && { [ -z "$v6" ] || ! printf ' %s ' "$aaaa" | grep -q " $v6 "; }; then
      problem="aaaa"; warn dns_aaaa "$DOMAIN" "$aaaa"
    fi
    if [ -z "$problem" ]; then
      info dns_ok "$DOMAIN" "$a"
      return 0
    fi
    [ "$said_yes" = 1 ] && { info dns_you_said; said_yes=0; }
    if ! interactive; then
      warn dns_skip_warn
      return 0
    fi
    info dns_howto "$DOMAIN" "$ip"
    case "$(choice "$(msg dns_options)" 4 1)" in
      1) continue ;;
      2) warn dns_skip_warn; return 0 ;;
      3) MODE=http; DOMAIN=""; return 0 ;;
      *) info aborted; exit 0 ;;
    esac
  done
}

# The provider's cloud firewall cannot be seen from here: ask about it.
check_cloud_firewall() {
  if interactive && ! confirm "$(msg fw_q)" y; then
    info fw_help
    confirm "$(msg continue_q)" y || { info aborted; exit 0; }
  fi
}

choose_updates() {
  [ -n "$SOURCE" ] && { AUTO_UPDATE=0; return; }
  [ -n "$AUTO_UPDATE" ] && return
  # Off unless the person installing says yes.
  AUTO_UPDATE=0
  if interactive && confirm "$(fmt au_q "$REPO")" n; then AUTO_UPDATE=1; fi
}

latest_release() {
  curl -fsS --max-time 10 -H 'Accept: application/vnd.github+json' "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
    | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1 || true
}

choose_version() {
  [ -n "$SOURCE" ] && { VERSION="source"; return; }
  [ -n "$VERSION" ] && return
  local tag; tag="$(latest_release)"
  if [ -n "$tag" ]; then VERSION="${tag#v}"; info version_latest "$VERSION"; else VERSION="latest"; warn version_none; fi
}

summary_and_confirm() {
  interactive || return 0
  echo
  info summary
  if [ "$MODE" = https ]; then info s_mode_https "$DOMAIN"; else info s_mode_http; fi
  info s_version "$VERSION"
  if [ "$AUTO_UPDATE" = 1 ]; then info s_au_on; else info s_au_off; fi
  info s_dir "$DIR"
  echo
  info plan_title
  [ "$PLAN_DOCKER" = 0 ] || info p_docker
  [ -z "$PLAN_FW" ] || info p_fw "$(fw_ports)" "$PLAN_FW"
  [ "$PLAN_SWAP" -eq 0 ] || info p_swap "$PLAN_SWAP" "$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)"
  in_container || info p_sysctl
  info p_trap "$DIR"
  echo
  confirm "$(msg install_q)" y || { info aborted; exit 0; }
}

# ---------------------------------------------------------------------------
# The system: one behaviour on every distribution
# ---------------------------------------------------------------------------

OS_NAME="Linux"; OS_ID=""; OS_LIKE=""; PM=""; INIT="none"
CHANGES=()            # kind|detail, reported at the end and undone by uninstall
PLAN_FW=""            # firewall tool that needs our ports opened
PLAN_SWAP=0           # MB of swap to create
PLAN_DOCKER=0

detect_system() {
  if [ -r /etc/os-release ]; then
    # Read in a subshell: os-release defines VERSION, NAME and others that
    # must not leak into the installer's own settings.
    # shellcheck disable=SC1091
    OS_NAME="$(. /etc/os-release && echo "${PRETTY_NAME:-${NAME:-Linux}}")"
    # shellcheck disable=SC1091
    OS_ID="$(. /etc/os-release && echo "${ID:-}")"
    # shellcheck disable=SC1091
    OS_LIKE="$(. /etc/os-release && echo "${ID_LIKE:-}")"
  fi
  local m
  for m in apt-get dnf yum zypper pacman apk; do
    if command -v "$m" >/dev/null; then PM="$m"; break; fi
  done
  if [ -d /run/systemd/system ] && command -v systemctl >/dev/null; then INIT=systemd
  elif command -v rc-service >/dev/null; then INIT=openrc
  fi
}

in_container() {
  [ -f /.dockerenv ] || [ -f /run/.containerenv ] || grep -qaE 'docker|lxc|kubepods' /proc/1/cgroup 2>/dev/null \
    || { command -v systemd-detect-virt >/dev/null && systemd-detect-virt -cq; }
}

record() { CHANGES+=("$1|$2"); }

pkg_install() { # quietly; true when every package installed
  case "$PM" in
    apt-get) DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "$@" >/dev/null 2>&1 ;;
    dnf|yum) "$PM" install -y -q "$@" >/dev/null 2>&1 ;;
    zypper) zypper --non-interactive --quiet install --no-recommends "$@" >/dev/null 2>&1 ;;
    # Arch supports no partial upgrades: sync and upgrade with the install.
    pacman) pacman -Syu --noconfirm --needed "$@" >/dev/null 2>&1 ;;
    apk) apk add -q "$@" >/dev/null 2>&1 ;;
    *) return 1 ;;
  esac
}

pkg_refresh() {
  case "$PM" in
    apt-get) apt-get update -qq >/dev/null 2>&1 || true ;;
    apk) apk update -q >/dev/null 2>&1 || true ;;
  esac
}

service_up() {
  case "$INIT" in
    systemd) systemctl enable --now "$1" >/dev/null 2>&1 ;;
    openrc) rc-update add "$1" default >/dev/null 2>&1; rc-service "$1" start >/dev/null 2>&1 ;;
    *) command -v service >/dev/null && service "$1" start >/dev/null 2>&1 ;;
  esac
}

compose_ok() { docker compose version >/dev/null 2>&1; }

# Docker Compose v2 as a CLI plugin, straight from its releases, checked
# against the published SHA-256: for distributions that ship Docker without it.
install_compose_plugin() {
  say compose_get
  local arch tag dir url sum
  case "$(uname -m)" in x86_64|amd64) arch=x86_64 ;; aarch64|arm64) arch=aarch64 ;; *) return 1 ;; esac
  tag="$(curl -fsS --max-time 10 https://api.github.com/repos/docker/compose/releases/latest | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1 || true)"
  [ -n "$tag" ] || return 1
  dir=/usr/local/lib/docker/cli-plugins
  url="https://github.com/docker/compose/releases/download/$tag/docker-compose-linux-$arch"
  mkdir -p "$dir"
  curl -fsSL --max-time 120 "$url" -o "$dir/docker-compose.new" || return 1
  sum="$(curl -fsSL --max-time 30 "$url.sha256" | awk '{print $1}' || true)"
  if [ -z "$sum" ] || [ "$(sha256sum "$dir/docker-compose.new" | awk '{print $1}')" != "$sum" ]; then
    rm -f "$dir/docker-compose.new"; die compose_bad
  fi
  install -m 0755 "$dir/docker-compose.new" "$dir/docker-compose"
  rm -f "$dir/docker-compose.new"
  record compose "$tag $dir/docker-compose"
}

# Docker's own repository, for distributions that package no Docker at all
# (RHEL and its clones, CentOS Stream, older Debian-likes).
add_docker_repo() {
  say docker_repo
  local family="$OS_ID"
  case "$PM" in
    dnf|yum)
      case "$OS_ID" in fedora) family=fedora ;; rhel) family=rhel ;; *) family=centos ;; esac
      pkg_install dnf-plugins-core || pkg_install yum-utils || true
      local repo="https://download.docker.com/linux/$family/docker-ce.repo"
      { "$PM" config-manager --add-repo "$repo" || "$PM" config-manager addrepo --from-repofile="$repo"; } >/dev/null 2>&1 || return 1
      record repo "$repo"
      pkg_install docker-ce docker-ce-cli containerd.io docker-compose-plugin && record pkg "docker-ce docker-ce-cli containerd.io docker-compose-plugin"
      ;;
    apt-get)
      case " $OS_ID $OS_LIKE " in *" ubuntu "*) family=ubuntu ;; *) family=debian ;; esac
      pkg_install ca-certificates curl gnupg || true
      install -m 0755 -d /etc/apt/keyrings
      curl -fsSL "https://download.docker.com/linux/$family/gpg" -o /etc/apt/keyrings/docker.asc || return 1
      chmod a+r /etc/apt/keyrings/docker.asc
      local codename
      # shellcheck disable=SC1091
      codename="$(. /etc/os-release && echo "${VERSION_CODENAME:-}")"
      echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/$family $codename stable" \
        > /etc/apt/sources.list.d/docker.list
      record repo "/etc/apt/sources.list.d/docker.list"
      pkg_refresh
      pkg_install docker-ce docker-ce-cli containerd.io docker-compose-plugin && record pkg "docker-ce docker-ce-cli containerd.io docker-compose-plugin"
      ;;
    *) return 1 ;;
  esac
}

# A real Docker Engine, not podman's emulation of its command line.
docker_ok() { command -v docker >/dev/null && ! docker --version 2>&1 | grep -qi podman; }

plan_docker() { if docker_ok && compose_ok; then PLAN_DOCKER=0; else PLAN_DOCKER=1; fi; }

# Docker Engine and Compose v2: the distribution's packages first, the
# Compose plugin from its releases where only it is missing, Docker's own
# repository where the distribution has no Docker at all.
ensure_docker() {
  [ "$PLAN_DOCKER" = 1 ] || return 0
  [ "$INSTALL_DOCKER" = 1 ] || die need_docker
  say docker_install
  pkg_refresh
  local engine="" compose=""
  case "$PM" in
    apt-get) engine="docker.io"; compose="docker-compose-v2 docker-compose docker-compose-plugin" ;;
    dnf|yum)
      # Fedora packages Docker as moby-engine, Amazon Linux as docker; on
      # RHEL and its clones "docker" is podman's emulation, so they go to
      # Docker's own repository instead.
      case "$OS_ID" in
        fedora) engine="moby-engine" ;;
        amzn) engine="docker" ;;
        *) engine="" ;;
      esac
      compose="docker-compose docker-compose-plugin" ;;
    zypper) engine="docker"; compose="docker-compose" ;;
    pacman) engine="docker"; compose="docker-compose" ;;
    apk) engine="docker"; compose="docker-cli-compose" ;;
  esac
  local pkg
  if ! docker_ok; then
    for pkg in $engine; do pkg_install "$pkg" && { record pkg "$pkg"; break; }; done
  fi
  docker_ok || add_docker_repo || true
  docker_ok || die docker_missing
  for pkg in $compose; do
    compose_ok && break
    pkg_install "$pkg" && record pkg "$pkg"
  done
  compose_ok || install_compose_plugin || true
  compose_ok || die docker_missing
  if service_up docker; then record service docker; fi
}

# Host firewall: whichever is active gets our ports, with rules marked so
# they can be found and removed again.
plan_firewall() {
  PLAN_FW=""
  if command -v ufw >/dev/null && ufw status 2>/dev/null | grep -q '^Status: active'; then PLAN_FW=ufw
  elif command -v firewall-cmd >/dev/null && firewall-cmd --state >/dev/null 2>&1; then PLAN_FW=firewalld
  elif command -v nft >/dev/null && nft list chains 2>/dev/null | grep -q 'hook input.*policy drop'; then PLAN_FW=nftables
  elif command -v iptables >/dev/null && iptables -S INPUT 2>/dev/null | grep -qE '^-P INPUT DROP|-j (REJECT|DROP)( |$)'; then PLAN_FW=iptables
  fi
}

fw_ports() { if [ "$MODE" = http ]; then echo 80; else echo "80 443"; fi; }

persist_iptables() {
  if command -v netfilter-persistent >/dev/null; then netfilter-persistent save >/dev/null 2>&1
  elif [ -d /etc/iptables ]; then iptables-save > /etc/iptables/rules.v4
  elif [ -f /etc/sysconfig/iptables ]; then iptables-save > /etc/sysconfig/iptables
  fi
}

open_firewall() {
  [ -n "$PLAN_FW" ] || return 0
  local p ports; ports="$(fw_ports)"
  case "$PLAN_FW" in
    ufw) for p in $ports; do ufw allow "$p/tcp" comment bot-cooker >/dev/null; done ;;
    firewalld)
      for p in $ports; do firewall-cmd --permanent --add-port="$p/tcp" >/dev/null; done
      firewall-cmd --reload >/dev/null ;;
    nftables)
      local fam table chain
      read -r fam table chain < <(nft list chains 2>/dev/null | awk '/^table/ {f=$2; t=$3} /chain/ {c=$2} /hook input.*policy drop/ {print f, t, c; exit}')
      nft insert rule "$fam" "$table" "$chain" tcp dport "{ ${ports// /, } }" accept comment '"bot-cooker"'
      for p in /etc/nftables.conf /etc/sysconfig/nftables.conf; do
        [ -f "$p" ] && { cp -n "$p" "$p.before-bot-cooker"; nft list ruleset > "$p"; break; }
      done ;;
    iptables)
      for p in $ports; do
        iptables -C INPUT -p tcp --dport "$p" -m comment --comment bot-cooker -j ACCEPT 2>/dev/null \
          || iptables -I INPUT -p tcp --dport "$p" -m comment --comment bot-cooker -j ACCEPT
      done
      persist_iptables ;;
  esac
  record fw "$PLAN_FW $ports"
}

close_firewall() { # close_firewall TOOL PORTS...
  local tool="$1" p; shift
  case "$tool" in
    ufw) for p in "$@"; do ufw delete allow "$p/tcp" >/dev/null 2>&1 || true; done ;;
    firewalld) for p in "$@"; do firewall-cmd --permanent --remove-port="$p/tcp" >/dev/null 2>&1 || true; done; firewall-cmd --reload >/dev/null 2>&1 || true ;;
    nftables)
      local fam table chain handle
      nft -a list ruleset 2>/dev/null | awk '/^table/ {f=$2; t=$3} /chain/ {c=$2} /comment "bot-cooker"/ {print f, t, c, $NF}' \
        | while read -r fam table chain handle; do nft delete rule "$fam" "$table" "$chain" handle "$handle" 2>/dev/null || true; done ;;
    iptables)
      for p in "$@"; do while iptables -D INPUT -p tcp --dport "$p" -m comment --comment bot-cooker -j ACCEPT 2>/dev/null; do :; done; done
      persist_iptables ;;
  esac
}

# Swap for small servers: image pulls and builds need memory headroom.
plan_swap() {
  PLAN_SWAP=0
  in_container && return 0
  local mem swap
  mem="$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo 2>/dev/null || echo 4096)"
  swap="$(awk '/SwapTotal/ {print int($2/1024)}' /proc/meminfo 2>/dev/null || echo 0)"
  [ "$swap" -eq 0 ] && [ "$mem" -lt 2048 ] || return 0
  [ -e /swapfile ] && return 0
  if [ "$mem" -lt 1024 ]; then PLAN_SWAP=2048; else PLAN_SWAP=1024; fi
}

make_swap() {
  [ "$PLAN_SWAP" -gt 0 ] || return 0
  { fallocate -l "${PLAN_SWAP}M" /swapfile 2>/dev/null || dd if=/dev/zero of=/swapfile bs=1M count="$PLAN_SWAP" status=none; } || return 0
  chmod 600 /swapfile
  if mkswap /swapfile >/dev/null 2>&1 && swapon /swapfile 2>/dev/null; then
    grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
    record swap "/swapfile $PLAN_SWAP"
  else
    rm -f /swapfile
  fi
}

SYSCTL_FILE=/etc/sysctl.d/90-bot-cooker.conf

# Kernel limits for many long-lived connections. Only ever raised: a value
# already set higher on this server is left alone.
tune_network() {
  in_container && return 0
  command -v sysctl >/dev/null || return 0
  local key want have out=""
  while read -r key want; do
    have="$(sysctl -n "$key" 2>/dev/null)" || continue
    [ "${have:-0}" -lt "$want" ] 2>/dev/null || continue
    out+="$key = $want"$'\n'
  done <<'EOF'
net.core.somaxconn 4096
net.ipv4.tcp_max_syn_backlog 8192
net.core.netdev_max_backlog 4096
fs.file-max 1048576
fs.nr_open 1048576
EOF
  [ -n "$out" ] || return 0
  printf '# Written by the bot-cooker installer: room for many long-lived\n# connections. Removed by "bot-cooker-ctl uninstall".\n%s' "$out" > "$SYSCTL_FILE"
  sysctl -q -p "$SYSCTL_FILE" >/dev/null 2>&1 || true
  record sysctl "$SYSCTL_FILE"
}

# What the installation did, saved for the report and for uninstall.
save_changes() {
  local c
  for c in "${CHANGES[@]+"${CHANGES[@]}"}"; do
    grep -qxF "$c" "$DIR/changes" 2>/dev/null || printf '%s\n' "$c" >> "$DIR/changes"
  done
}

report_changes() {
  [ -f "$DIR/changes" ] || return 0
  echo
  info report_title
  local kind detail
  while IFS='|' read -r kind detail; do
    case "$kind" in
      pkg) info ch_pkg "$detail" ;;
      repo) info ch_repo "$detail" ;;
      compose) info ch_compose "$detail" ;;
      service) info ch_service "$detail" ;;
      fw) info ch_fw "${detail#* }" "${detail%% *}" ;;
      swap) info ch_swap "${detail%% *}" "${detail#* }" ;;
      sysctl) info ch_sysctl "$detail" ;;
      files) info ch_files "$detail" ;;
      ctl) info ch_ctl "$detail" ;;
      timer) info ch_timer "$detail" ;;
    esac
  done < "$DIR/changes"
}

# Undo what the installation changed in the system; Docker itself stays.
revert_changes() {
  [ -f "$DIR/changes" ] || return 0
  local kind detail docker=0
  while IFS='|' read -r kind detail; do
    case "$kind" in
      fw) # shellcheck disable=SC2086 # tool then ports
        close_firewall ${detail}; info reverted "$detail" ;;
      swap) swapoff /swapfile 2>/dev/null || true; sed -i '\#^/swapfile #d' /etc/fstab; rm -f /swapfile; info reverted "/swapfile" ;;
      sysctl) rm -f "$detail"; sysctl -q --system >/dev/null 2>&1 || true; info reverted "$detail" ;;
      pkg|repo|compose|service) docker=1 ;;
    esac
  done < "$DIR/changes"
  [ "$docker" = 0 ] || info docker_kept
}

# Connections the trap may hold on this server's memory: each costs the trap
# ~30 KiB and, over HTTPS, the Caddy sidecar ~250 KiB more. Past the cap the
# trap answers at once, which keeps both bounded.
max_connections() {
  local mem per n
  mem="$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo 2>/dev/null || echo 1024)"
  if [ "$MODE" = https ]; then per=300; else per=40; fi
  n=$(( (mem - 350) * 1024 / per ))
  [ "$n" -ge 500 ] || n=500
  [ "$n" -le 8000 ] || n=8000
  echo "$n"
}

write_files() {
  mkdir -p "$DIR"
  chmod 700 "$DIR"
  # Secrets are generated once and kept: the master key is the persona.
  umask 077
  touch "$DIR/.env"
  env_set() {
    if grep -q "^$1=" "$DIR/.env"; then sed -i "s|^$1=.*|$1=$2|" "$DIR/.env"; else printf '%s=%s\n' "$1" "$2" >> "$DIR/.env"; fi
  }
  [ -n "$(env_get COOKER_MASTER_KEY)" ] || env_set COOKER_MASTER_KEY "$(od -An -tx1 -N32 /dev/urandom | tr -d ' \n')"
  [ -n "$(env_get COOKER_ADMIN_TOKEN)" ] || env_set COOKER_ADMIN_TOKEN "$(od -An -tx1 -N24 /dev/urandom | tr -d ' \n')"
  env_set DOMAIN "$DOMAIN"
  env_set COOKER_IMAGE "$IMAGE:$VERSION"
  cat > "$CONF" <<EOF
# Answers of the last installation; bot-cooker-ctl and updates reuse them.
DOMAIN='$DOMAIN'
MODE='$MODE'
REPO='$REPO'
IMAGE='$IMAGE'
VERSION='$VERSION'
SOURCE='$SOURCE'
PACK='$PACK'
AUTO_UPDATE='$AUTO_UPDATE'
UI_LANG='$UI_LANG'
EOF
  umask 022

  local image_block pack_block=""
  if [ -n "$SOURCE" ]; then
    image_block="    build: $SOURCE
    image: bot-cooker:local"
  else
    image_block="    image: \${COOKER_IMAGE}
    pull_policy: always"
  fi
  if [ -n "$PACK" ]; then
    pack_block="    volumes:
      - $PACK:/app/pack:ro"
  fi
  local service
  service="  bot-cooker:
$image_block
    restart: unless-stopped
    environment:
      COOKER_MASTER_KEY: \${COOKER_MASTER_KEY}
      COOKER_ADMIN_TOKEN: \${COOKER_ADMIN_TOKEN}
      COOKER_TEMPLATES_DIR: ${PACK:+/app/pack}
      COOKER_MAX_CONCURRENT: \"$(max_connections)\"
$pack_block
    ulimits:
      nofile: {soft: 65535, hard: 65535}
    logging:
      driver: json-file
      options: {max-size: 10m, max-file: \"3\"}"

  if [ "$MODE" = http ]; then
    cat > "$DIR/compose.yml" <<EOF
# Written by the bot-cooker installer; re-run it instead of editing.
services:
$service
    ports: ["80:8080"]
EOF
  else
    cat > "$DIR/Caddyfile" <<'EOF'
# Written by the bot-cooker installer; re-run it instead of editing.
{
	# No HTTP/3: it would add an Alt-Svc header no persona's server sends.
	servers {
		protocols h1 h2
	}
}

{$DOMAIN} {
	# The persona's own Server header passes through; Caddy's Via would not fit.
	header -Via
	reverse_proxy bot-cooker:8080 {
		# Stream the slow responses as they are produced.
		flush_interval -1
		header_up X-Real-IP {remote_host}
	}
}
EOF
    cat > "$DIR/compose.yml" <<EOF
# Written by the bot-cooker installer; re-run it instead of editing.
services:
$service
    expose: ["8080"]
    networks: [net]

  caddy:
    image: caddy:2-alpine
    restart: unless-stopped
    ports: ["80:80", "443:443"]
    environment:
      DOMAIN: \${DOMAIN}
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy_data:/data
      - caddy_config:/config
    networks: [net]
    depends_on: [bot-cooker]
    logging:
      driver: json-file
      options: {max-size: 10m, max-file: "3"}

networks:
  net:

volumes:
  caddy_data:
  caddy_config:
EOF
  fi
}

install_tools() {
  # This script becomes the control tool; updates replace it.
  local self="${BASH_SOURCE[0]:-}"
  if [ -n "$self" ] && [ -f "$self" ] && [ "$(readlink -f "$self")" != "$(readlink -f "$DIR/install.sh" 2>/dev/null || true)" ]; then
    install -m 0700 "$self" "$DIR/install.sh"
  elif [ ! -f "$DIR/install.sh" ] || [ -z "$self" ] || [ ! -f "$self" ]; then
    # Piped from curl: fetch the same file from the repository.
    if curl -fsSL "https://raw.githubusercontent.com/$REPO/main/deploy/install.sh" -o "$DIR/install.sh.new" 2>/dev/null; then
      install -m 0700 "$DIR/install.sh.new" "$DIR/install.sh"
    else
      warn ctl_missing
    fi
    rm -f "$DIR/install.sh.new"
  fi
  cat > "$CTL" <<EOF
#!/bin/sh
# Control bot-cooker: status, stats, logs, stop, start, restart, update,
# auto-update on|off, uninstall, guide.
[ "\$(id -u)" -eq 0 ] || exec sudo "\$0" "\$@"
exec bash "$DIR/install.sh" "\${@:-status}"
EOF
  chmod 0755 "$CTL"
  write_guide
}

set_auto_update() {
  if [ "$AUTO_UPDATE" = 1 ] && command -v systemctl >/dev/null && [ -d /run/systemd/system ]; then
    cat > "/etc/systemd/system/$UNIT.service" <<EOF
[Unit]
Description=Check for and install new bot-cooker releases
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/bin/bash $DIR/install.sh update --auto
EOF
    cat > "/etc/systemd/system/$UNIT.timer" <<EOF
[Unit]
Description=Daily bot-cooker update check

[Timer]
OnCalendar=daily
# Spread over the day so instances do not update in lockstep.
RandomizedDelaySec=12h
Persistent=true

[Install]
WantedBy=timers.target
EOF
    systemctl daemon-reload
    systemctl enable --now "$UNIT.timer" >/dev/null 2>&1
    rm -f /etc/cron.d/bot-cooker-update
    record timer "$UNIT.timer (systemd)"
  elif [ "$AUTO_UPDATE" = 1 ]; then
    # Without systemd: cron, at a random time of day of this server's own.
    local when; when="$(( RANDOM % 60 )) $(( RANDOM % 24 )) * * *"
    if [ -d /etc/cron.d ]; then
      printf '%s root /bin/bash %s/install.sh update --auto >/dev/null 2>&1\n' "$when" "$DIR" > /etc/cron.d/bot-cooker-update
      record timer "/etc/cron.d/bot-cooker-update"
    elif [ -d /etc/crontabs ]; then
      sed -i '/# bot-cooker-update$/d' /etc/crontabs/root 2>/dev/null || true
      printf '%s /bin/bash %s/install.sh update --auto >/dev/null 2>&1 # bot-cooker-update\n' "$when" "$DIR" >> /etc/crontabs/root
      record timer "/etc/crontabs/root"
    else
      warn au_no_scheduler
      AUTO_UPDATE=0
      [ -f "$CONF" ] && sed -i "s/^AUTO_UPDATE=.*/AUTO_UPDATE='0'/" "$CONF"
    fi
  else
    if command -v systemctl >/dev/null && [ -f "/etc/systemd/system/$UNIT.timer" ]; then
      systemctl disable --now "$UNIT.timer" >/dev/null 2>&1 || true
      rm -f "/etc/systemd/system/$UNIT.timer" "/etc/systemd/system/$UNIT.service"
      systemctl daemon-reload
    fi
    rm -f /etc/cron.d/bot-cooker-update
    [ -f /etc/crontabs/root ] && sed -i '/# bot-cooker-update$/d' /etc/crontabs/root
    true
  fi
}

start_trap() {
  if [ -n "$SOURCE" ]; then
    say building "$SOURCE"
    compose build --pull
  else
    say pulling "$IMAGE:$VERSION"
    compose pull --quiet 2>/dev/null || compose pull || die pull_failed "$IMAGE:$VERSION" "$REPO"
  fi
  say starting
  compose up -d --remove-orphans
}

# Wait for the trap to answer and, on HTTPS, for a certificate that verifies
# (which proves Let's Encrypt reached this server from the internet). On
# failure, read Caddy's log and say what to fix.
verify() {
  local url opts=()
  if [ "$MODE" = http ]; then
    url="http://127.0.0.1/robots.txt"; say waiting
  else
    url="https://$DOMAIN/robots.txt"; opts=(--resolve "$DOMAIN:443:127.0.0.1"); say waiting_cert
  fi
  local i
  for i in $(seq 1 36); do
    if curl -fsS -o /dev/null --max-time 15 ${opts[@]+"${opts[@]}"} "$url" 2>/dev/null; then
      say ok_running
      local persona
      persona="$(compose logs bot-cooker 2>/dev/null | grep 'INFO persona:' | tail -n 1 | sed -e 's/.*persona: //' -e 's/ | .*//' || true)"
      [ -z "$persona" ] || info persona_is "$persona"
      return 0
    fi
    # A trap container that keeps restarting will not get better by waiting.
    if [ "$i" -ge 6 ] && compose ps bot-cooker 2>/dev/null | grep -qi restarting; then
      warn fail_trap
      compose logs --tail 15 bot-cooker >&2 || true
      return 1
    fi
    sleep 5
  done
  diagnose
  return 1
}

diagnose() {
  if [ "$MODE" = https ]; then
    local log; log="$(compose logs --tail 200 caddy 2>/dev/null || true)"
    if printf '%s' "$log" | grep -qiE 'rateLimited|too many certificates'; then warn fail_rate; return; fi
    if printf '%s' "$log" | grep -qiE 'NXDOMAIN|no valid A records|DNS problem'; then warn fail_dns "$DOMAIN"; return; fi
    if printf '%s' "$log" | grep -qiE 'Timeout during connect|likely firewall|Connection refused'; then warn fail_fw; return; fi
    if printf '%s' "$log" | grep -qiE 'unauthorized|Invalid response from'; then warn fail_other_host "$DOMAIN"; return; fi
  fi
  if compose ps bot-cooker 2>/dev/null | grep -qi restarting; then
    warn fail_trap
    compose logs --tail 15 bot-cooker >&2 || true
    return
  fi
  warn fail_generic
}

# ---------------------------------------------------------------------------
# The guide for whoever installed it
# ---------------------------------------------------------------------------

write_guide() {
  local base
  if [ "$MODE" = https ]; then base="https://$DOMAIN"; else base="http://<IP>"; fi
  if [ "$UI_LANG" = ru ]; then
    cat > "$DIR/GUIDE.txt" <<EOF
bot-cooker: как пользоваться
============================

Ловушка работает в фоне и сама перезапускается после перезагрузки сервера.
Все команды выполняются на сервере (sudo спросит пароль, если нужно).

Посмотреть, всё ли в порядке
  bot-cooker-ctl status      — работает ли ловушка, её персона и счётчики
  bot-cooker-ctl stats       — подробная статистика: сколько сканеров поймано
  bot-cooker-ctl logs        — журнал (выход: Ctrl+C)

Проверить снаружи
  $base/robots.txt  — должна открыться страница сразу.
  Остальные страницы открываются медленно — так и задумано.

Остановить и запустить
  bot-cooker-ctl stop        — остановить (до команды start)
  bot-cooker-ctl start       — запустить снова
  bot-cooker-ctl restart     — перезапустить (например, после смены DNS)

Обновления
  bot-cooker-ctl update            — проверить и установить новую версию сейчас
  bot-cooker-ctl auto-update on    — обновляться автоматически раз в сутки
  bot-cooker-ctl auto-update off   — выключить автообновления

Удалить
  bot-cooker-ctl uninstall         — удалить ловушку, сохранив ключ и сертификат
  bot-cooker-ctl uninstall --purge — удалить всё без следа

Где что лежит
  $DIR/.env          — секретный ключ и токен (никому не показывайте)
  $DIR/install.conf  — ответы установки
  $DIR/update.log    — журнал автообновлений

Документация: https://koval01.github.io/bot-cooker/
EOF
  else
    cat > "$DIR/GUIDE.txt" <<EOF
bot-cooker: how to use it
=========================

The trap runs in the background and starts again after a reboot by itself.
Run these commands on the server (sudo asks for your password if needed).

Is everything all right?
  bot-cooker-ctl status      - is the trap running, its persona and counters
  bot-cooker-ctl stats       - detailed statistics: how many scanners it held
  bot-cooker-ctl logs        - the log (leave with Ctrl+C)

Check from outside
  $base/robots.txt  - should open at once.
  Other pages open slowly: that is the point.

Stop and start
  bot-cooker-ctl stop        - stop it (until you start it again)
  bot-cooker-ctl start       - start it again
  bot-cooker-ctl restart     - restart it (e.g. after a DNS change)

Updates
  bot-cooker-ctl update            - check for and install a new version now
  bot-cooker-ctl auto-update on    - update automatically once a day
  bot-cooker-ctl auto-update off   - stop automatic updates

Remove it
  bot-cooker-ctl uninstall         - remove the trap, keep its key and certificate
  bot-cooker-ctl uninstall --purge - remove everything

Where things are
  $DIR/.env          - the secret key and token (show them to no one)
  $DIR/install.conf  - the installation's answers
  $DIR/update.log    - automatic update log

Documentation: https://koval01.github.io/bot-cooker/
EOF
  fi
  chmod 0644 "$DIR/GUIDE.txt"
}

# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

stats_json() {
  local token url opts=()
  token="$(env_get COOKER_ADMIN_TOKEN)"
  if [ "$MODE" = https ]; then
    url="https://$DOMAIN/_cooker/stats"; opts=(--resolve "$DOMAIN:443:127.0.0.1")
  else
    url="http://127.0.0.1/_cooker/stats"
  fi
  # The token goes in on stdin, never on the command line.
  curl -fsS --max-time 10 ${opts[@]+"${opts[@]}"} -H @- "$url" <<<"X-Admin-Token: $token" 2>/dev/null
}

field() { sed -n "s/^[[:space:]]*\"$1\":[[:space:]]*\([0-9]*\).*/\1/p" | head -n 1; }

cmd_status() {
  installed || die not_installed
  compose ps --format '{{.Service}}: {{.State}} ({{.Status}})' 2>/dev/null || compose ps
  local persona json
  persona="$(compose logs bot-cooker 2>/dev/null | grep 'INFO persona:' | tail -n 1 | sed -e 's/.*persona: //' -e 's/ | .*//' || true)"
  [ -z "$persona" ] || info persona_is "$persona"
  if [ "$MODE" = https ]; then info s_mode_https "$DOMAIN"; else info s_mode_http; fi
  info s_version "${INSTALLED_VERSION:-?}"
  if [ "$AUTO_UPDATE" = 1 ]; then info s_au_on; else info s_au_off; fi
  if json="$(stats_json)"; then
    printf '  active %s | requests %s | clients %s | bytes %s | held %ss\n' \
      "$(field active <<<"$json")" "$(field requests_total <<<"$json")" "$(field unique_ips <<<"$json")" \
      "$(field bytes_fed <<<"$json")" "$(field wasted_secs <<<"$json")"
  fi
}

cmd_update() {
  installed || die not_installed
  [ -z "$SOURCE" ] || { cmd_reinstall; return; }
  local tag ver tmp
  tag="$(latest_release)"
  if [ -z "$tag" ]; then log update_check_failed; return 1; fi
  ver="${tag#v}"
  # A non-release tag ("latest" before the first release) is always older.
  local current="$INSTALLED_VERSION"
  case "$current" in [0-9]*) ;; *) current="0" ;; esac
  if [ "$ver" = "$current" ] || [ "$(printf '%s\n%s\n' "$ver" "$current" | sort -V | tail -n 1)" != "$ver" ]; then
    log up_to_date "${INSTALLED_VERSION:-$ver}"
    return 0
  fi
  log update_found "$ver" "${INSTALLED_VERSION:-?}"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  local base="https://github.com/$REPO/releases/download/$tag"
  if ! curl -fsSL --max-time 60 "$base/install.sh" -o "$tmp/install.sh" || ! curl -fsSL --max-time 60 "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"; then
    log update_check_failed; return 1
  fi
  if ! (cd "$tmp" && grep ' install.sh$' SHA256SUMS | sha256sum -c --status); then
    log update_bad_sum; return 1
  fi
  # The new installer replaces this one; keep it to roll back with.
  local previous="$INSTALLED_VERSION"
  cp "$DIR/install.sh" "$tmp/previous.sh"
  if bash "$tmp/install.sh" --yes --version "$ver"; then
    log update_done "$ver"
  else
    log update_rollback "$previous"
    bash "$tmp/previous.sh" --yes --version "$previous" || true
    install -m 0700 "$tmp/previous.sh" "$DIR/install.sh"
    return 1
  fi
}

cmd_reinstall() { ASSUME_YES=1; do_install; }

cmd_uninstall() {
  installed || { info not_installed; return 0; }
  if interactive; then
    confirm "$(msg uninstall_q)" n || { info aborted; exit 0; }
    [ "$PURGE" = 1 ] || { confirm "$(msg purge_q)" n && PURGE=1; } || true
  fi
  if [ "$PURGE" = 1 ]; then compose down --volumes --remove-orphans || true; else compose down --remove-orphans || true; fi
  AUTO_UPDATE=0
  set_auto_update
  rm -f "$CTL"
  detect_system
  revert_changes
  rm -f "$DIR/changes"
  if [ "$PURGE" = 1 ]; then
    rm -rf "$DIR"
    info removed
  else
    info removed_kept "$DIR"
  fi
}

do_install() {
  if interactive; then
    echo; info welcome; echo
    confirm "$(msg continue_q)" y || { info aborted; exit 0; }
    if installed; then
      info existing "$DIR"
      case "$(choice "$(msg existing_menu)" 3 1)" in
        2) cmd_uninstall; exit 0 ;;
        3) info aborted; exit 0 ;;
      esac
    fi
  fi
  if [ -n "$SOURCE" ]; then
    SOURCE="$(cd "$SOURCE" && pwd)" || exit 1
    [ -f "$SOURCE/Dockerfile" ] || die "$SOURCE: no Dockerfile"
  fi
  if [ -n "$PACK" ]; then PACK="$(cd "$PACK" && pwd)" || exit 1; fi
  # Questions and checks first; nothing on the server changes before the
  # summary is confirmed.
  detect_system
  interactive && info os_is "$OS_NAME"
  command -v curl >/dev/null || { pkg_refresh; pkg_install curl ca-certificates || pkg_install curl; } || true
  choose_mode
  choose_domain
  preflight
  check_dns
  check_cloud_firewall
  choose_updates
  choose_version
  plan_docker
  plan_firewall
  plan_swap
  summary_and_confirm
  make_swap
  ensure_docker
  tune_network
  open_firewall
  write_files
  record files "$DIR"
  start_trap
  install_tools
  record ctl "$CTL"
  set_auto_update
  save_changes
  local ok=0
  verify && ok=1
  echo
  cat "$DIR/GUIDE.txt"
  report_changes
  [ "$ok" = 1 ]
}

case "$COMMAND" in
  install) do_install ;;
  status) cmd_status ;;
  stats) installed || die not_installed; stats_json || die fail_generic; echo ;;
  logs) installed || die not_installed; compose logs -f --tail 100 ;;
  stop) installed || die not_installed; compose stop; info stopped ;;
  start) installed || die not_installed; compose up -d; info started ;;
  restart) installed || die not_installed; compose restart; info started ;;
  update) cmd_update ;;
  guide) if [ -f "$DIR/GUIDE.txt" ]; then cat "$DIR/GUIDE.txt"; else die not_installed; fi ;;
  auto-update)
    installed || die not_installed
    case "${ARGS[0]:-}" in
      on) AUTO_UPDATE=1 ;;
      off) AUTO_UPDATE=0 ;;
      *) usage; exit 2 ;;
    esac
    [ -z "$SOURCE" ] || AUTO_UPDATE=0
    sed -i "s/^AUTO_UPDATE=.*/AUTO_UPDATE='$AUTO_UPDATE'/" "$CONF"
    set_auto_update
    if [ "$AUTO_UPDATE" = 1 ]; then info au_on; else info au_off; fi
    ;;
  uninstall) cmd_uninstall ;;
esac
