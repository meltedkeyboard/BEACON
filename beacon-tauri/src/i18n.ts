// Two-language (English/Russian) UI translation. Static markup declares its own keys via
// `data-i18n`/`data-i18n-placeholder`/`data-i18n-title`/`data-i18n-aria-label` attributes in
// index.html; `applyI18n()` walks those on startup and again whenever the language changes.
// Dynamic strings built up in the feature modules call `t(key, vars?)` directly instead. There's
// no third-party i18n library here -- two languages and a few hundred short strings don't need
// one, and a flat key->string lookup keeps the translation surface (this file) in one place.

export type Lang = "en" | "ru";

const LANG_KEY = "beacon:lang";

type Dict = Record<string, string>;

const en: Dict = {
  "titlebar.minimize": "Minimize",
  "titlebar.maximize": "Maximize",
  "titlebar.close": "Close",

  "account.signin": "Sign in",
  "account.offline": "Offline mode",
  "account.menu.signin": "Sign in with Microsoft",
  "account.menu.addOffline": "Add offline account",
  "account.menu.manage": "Manage accounts",

  "nav.news": "News",
  "nav.javaEdition": "Java Edition",
  "nav.accounts": "Accounts",
  "nav.settings": "Settings",

  "tab.moments": "Moments",
  "tab.installations": "Installations",
  "tab.skins": "Skins",
  "tab.notes": "Patch notes",

  "installations.new": "New instance",
  "installations.import": "Import",

  "skins.signinBody": "Sign in with a Microsoft account to manage skins and capes.",
  "skins.signinBtn": "Sign in with Microsoft",
  "skins.variant.classic": "Classic",
  "skins.variant.slim": "Slim",
  "skins.upload": "Upload skin…",
  "skins.reset": "Reset to default",
  "skins.capes": "Capes",
  "skins.refresh": "Refresh",

  "notes.placeholder": "Patch notes will render here.",

  "playbar.installing": "Installing…",
  "playbar.noInstances": "No instances",
  "playbar.play": "Play",
  "playbar.notSignedIn": "Not signed in",

  "accounts.title": "Accounts",
  "accounts.back": "← Back",
  "accounts.body": "The account on top is used for Play. Move an account up to make it current.",
  "accounts.addOffline": "Add offline",
  "accounts.addMicrosoft": "Add Microsoft",

  "settings.title": "Settings",
  "settings.back": "← Back",
  "settings.theme.label": "Theme",
  "settings.theme.hint": "Pick a color scheme.",
  "settings.theme.darkGreen": "Dark · Green",
  "settings.theme.darkAmber": "Dark · Amber",
  "settings.theme.lightGreen": "Light · Green",
  "settings.theme.lightAmber": "Light · Amber",
  "settings.theme.starlight": "Star Light",
  "settings.language.label": "Language",
  "settings.language.hint": "Interface language.",
  "settings.language.en": "English",
  "settings.language.ru": "Русский",
  "settings.snapshots.label": "Show snapshots and old versions",
  "settings.snapshots.hint": "Include snapshots, old betas and alphas in the version list.",
  "settings.moments.label": "Moments tab",
  "settings.moments.hint": "Show a \"Moments\" tab with a slideshow of the selected instance's own screenshots.",
  "settings.momentsBg.label": "Moments background",
  "settings.momentsBg.hint": "Show the selected instance's own screenshots behind the Moments tab.",
  "settings.blur.label": "Background blur",
  "settings.blur.hint": "How much to blur the Moments tab's screenshot background.",
  "settings.gameData.label": "Game data",
  "settings.gameData.hint": "Downloaded versions, libraries and assets. Shared by every instance.",
  "settings.instances.label": "Instances",
  "settings.instances.hint": "Each instance's own saves, resource packs, shader packs and config.",
  "settings.curseforge.label": "CurseForge API key",
  "settings.curseforge.hintPrefix":
    "Optional -- paste your own personal CurseForge API key to enable browsing CurseForge mods, resource packs, and shader packs (Beacon can't ship a shared key; CurseForge's own terms forbid embedding one in a distributed app).",
  "settings.curseforge.notSet": "Not set.",
  "settings.curseforge.set": "Key set.",
  "settings.curseforge.placeholder": "Paste API key",
  "settings.curseforge.save": "Save",
  "settings.curseforge.clear": "Clear",
  "settings.curseforge.request": "Get a key…",
  "settings.config.label": "Launcher config",
  "settings.config.hint": "Accounts, instance list and these settings. Not relocatable from here.",
  "settings.wipe.label": "Wipe all data",
  "settings.wipe.hint": "Deletes every account, instance, world and downloaded file, then closes Beacon. Can't be undone.",
  "settings.wipe.button": "Wipe everything",
  "settings.open": "Open",
  "settings.browse": "Browse…",
  "settings.moving": "Moving...",
  "settings.unknown": "Unknown",
  "toggle.on": "On",
  "toggle.off": "Off",

  "instance.back": "← Back",
  "instance.title": "Instance",
  "instance.changeIcon": "Change icon",
  "instance.start": "Start",
  "instance.stop": "Stop",
  "instance.rename": "Rename",
  "instance.delete": "Delete",
  "instance.moreActions": "More actions",
  "instance.clearIcon": "Clear icon",
  "instance.openFolder": "Open .minecraft",
  "instance.openLibraries": "Open libraries",
  "instance.export": "Export",

  "instance.tab.overview": "Overview",
  "instance.tab.mods": "Mods",
  "instance.tab.worlds": "Worlds",
  "instance.tab.resourcePacks": "Resource Packs",
  "instance.tab.shaderPacks": "Shader Packs",
  "instance.tab.screenshots": "Screenshots",
  "instance.tab.advanced": "Advanced",

  "instance.version.title": "Version",
  "instance.version.change": "Change version",
  "instance.loader.installEllipsis": "Install…",
  "instance.loader.changeEllipsis": "Change…",
  "instance.loader.remove": "Remove",

  "instance.mods.title": "Mods",
  "instance.mods.openFolder": "Open folder",
  "instance.mods.browse": "Browse mods…",
  "instance.mods.needLoader": "Install a mod loader first",
  "instance.mods.delete": "Delete",
  "instance.mods.add": "Add mod",
  "instance.mods.empty": "No mods yet.",
  "instance.worlds.title": "Worlds",
  "instance.worlds.openFolder": "Open folder",
  "instance.worlds.empty": "No worlds yet.",
  "instance.resourcePacks.title": "Resource Packs",
  "instance.resourcePacks.openFolder": "Open folder",
  "instance.resourcePacks.browse": "Browse resource packs…",
  "instance.resourcePacks.empty": "No resource packs yet.",
  "instance.shaderPacks.title": "Shader Packs",
  "instance.shaderPacks.openFolder": "Open folder",
  "instance.shaderPacks.browse": "Browse shader packs…",
  "instance.shaderPacks.empty": "No shader packs yet.",
  "instance.screenshots.title": "Screenshots",
  "instance.screenshots.openFolder": "Open folder",
  "instance.screenshots.empty": "No screenshots yet.",
  "instance.advanced.log": "Game Log",
  "instance.advanced.clear": "Clear",
  "instance.advanced.noSession": "No active session for this instance.",
  "instance.advanced.title": "Advanced",
  "instance.advanced.body": "Low-level jar patching, MultiMC-style. Not implemented yet.",
  "instance.advanced.notImplemented": "Not implemented yet",
  "instance.advanced.addModToJar": "Add mod to jar-file Minecraft",
  "instance.advanced.replaceJar": "Replace Minecraft.jar",
  "instance.advanced.addAgents": "Add agents",
  "instance.advanced.addEmptyFile": "Add empty file",
  "instance.advanced.importComponents": "Import components",

  "login.title": "Sign in with Microsoft",
  "login.body": "Open the page below and enter this code to finish signing in.",
  "login.openBrowser": "Open browser",
  "login.close": "Close",

  "error.title": "Unexpected error",
  "error.close": "Close",

  "offline.title": "Add offline account",
  "offline.body": "Pick a nickname. It doesn't need to match your Microsoft account.",
  "offline.placeholder": "Nickname",
  "offline.add": "Add",
  "offline.cancel": "Cancel",

  "createInstance.title": "New instance",
  "createInstance.namePlaceholder": "Instance name",
  "createInstance.create": "Create",
  "createInstance.cancel": "Cancel",

  "changeVersion.title": "Change version",
  "changeVersion.cancel": "Cancel",

  "installLoader.title": "Install mod loader",
  "installLoader.installing": "Installing…",
  "installLoader.install": "Install",
  "installLoader.cancel": "Cancel",

  "browseContent.back": "← Back",
  "browseContent.modrinth": "Modrinth",
  "browseContent.curseforge": "CurseForge",
  "browseContent.curseforgeHint": "Paste a CurseForge API key in Settings to enable this source.",
  "browseContent.searchPlaceholder": "Search…",
  "browseContent.checkUpdates": "Check for updates",
  "browseContent.selectItem": "Select an item to see details.",
  "browseContent.reviewInstall": "Review & Install",
  "browseContent.reviewPlaceholder": "Check items on the left to review them here before installing.",
  "browseContent.installing": "Installing…",
  "browseContent.install": "Install (0)",

  "renameInstance.title": "Rename instance",
  "renameInstance.confirm": "Rename",
  "renameInstance.cancel": "Cancel",

  "deleteInstance.title": "Delete instance?",
  "deleteInstance.body": "This permanently deletes its worlds and everything else in its folder. This can't be undone.",
  "deleteInstance.confirm": "Delete",
  "deleteInstance.cancel": "Cancel",

  "wipe.title": "Wipe all data?",
  "wipe.body": "This permanently deletes every account, instance, world and downloaded file, then closes Beacon. This can't be undone. If a game is currently running, close it first.",
  "wipe.placeholder": "Type WIPE to confirm",
  "wipe.confirm": "Wipe everything",
  "wipe.wiping": "Wiping...",
  "wipe.cancel": "Cancel",

  "play.play": "Play",
  "play.signIn": "Sign In",
  "play.newInstance": "New instance",
  "play.selectInstance": "Select an instance",
  "play.installing": "Installing...",
  "play.checking": "Checking...",
  "play.checkingFiles": "Checking files...",
  "play.launching": "Launching...",
  "play.running": "Running...",
  "play.downloading": "Downloading",
  "play.checkingVerb": "Checking",
  "play.launchFailedPrefix": "Couldn't launch: {message}",

  "account.connecting": "Connecting...",
  "account.connectionFailed": "Connection failed. Please log in again.",
  "account.connected": "Connected",

  "instances.playbar.noInstances": "No instances",
  "instances.picker.selectInstance": "Select instance",
  "instances.picker.emptyList": "No instances yet.",
  "instances.grid.empty": "No instances yet. Create one to get started.",
  "instances.loaderNone": "Mod Loader — None",
  "instances.loaderNamed": "Mod Loader — {name}",
  "instances.versionPrefix": "Minecraft {version}",
  "instances.deleteBody": "This permanently deletes \"{name}\" -- its worlds and everything else in its folder. This can't be undone.",

  "modContent.updateTo": "Update to {version}",
  "modContent.updating": "Updating…",
  "modContent.removing": "Removing…",
  "modContent.remove": "Remove",
  "modContent.installedFmt": "Install ({count})",
  "modContent.checking": "Checking…",
  "modContent.startingInstall": "Starting…",
  "modContent.bringsIn": "Brings in: {list}",
  "modContent.removeConfirm": "Remove {noun}?",
  "modContent.removeBody": "This permanently deletes \"{filename}\". This can't be undone.",
  "modContent.noun.mod": "mod",
  "modContent.noun.resourcePack": "resource pack",
  "modContent.noun.shaderPack": "shader pack",
  "modContent.title.mod": "Browse mods",
  "modContent.title.resourcePack": "Browse resource packs",
  "modContent.title.shaderPack": "Browse shader packs",
  "modContent.placeholder.mod": "Search mods…",
  "modContent.placeholder.resourcePack": "Search resource packs…",
  "modContent.placeholder.shaderPack": "Search shader packs…",
  "modContent.empty.mod": "No mods found.",
  "modContent.empty.resourcePack": "No resource packs found.",
  "modContent.empty.shaderPack": "No shader packs found.",
  "modContent.source.unknown": "Unknown",

  "common.remove": "Remove",
  "common.unpin": "Unpin",
  "common.pinAsMomentsBg": "Pin as Moments tab background",
  "confirm.deleteFilePrefix": "This permanently deletes \"{name}\". This can't be undone.",
  "confirm.deleteWorld.title": "Delete world?",
  "confirm.deleteResourcePack.title": "Delete resource pack?",
  "confirm.deleteShaderPack.title": "Delete shader pack?",
  "confirm.deleteMods.title": "Delete mods?",
  "confirm.deleteModsBody.single": "This permanently deletes \"{name}\". This can't be undone.",
  "confirm.deleteModsBody.multi": "This permanently deletes {count} mods: {names}. This can't be undone.",
  "instance.mods.deleteFmt": "Delete ({count})",
};

const ru: Dict = {
  "titlebar.minimize": "Свернуть",
  "titlebar.maximize": "Развернуть",
  "titlebar.close": "Закрыть",

  "account.signin": "Войти",
  "account.offline": "Оффлайн-режим",
  "account.menu.signin": "Войти через Microsoft",
  "account.menu.addOffline": "Добавить оффлайн-аккаунт",
  "account.menu.manage": "Управление аккаунтами",

  "nav.news": "Новости",
  "nav.javaEdition": "Java Edition",
  "nav.accounts": "Аккаунты",
  "nav.settings": "Настройки",

  "tab.moments": "Моменты",
  "tab.installations": "Установки",
  "tab.skins": "Скины",
  "tab.notes": "Патч-ноты",

  "installations.new": "Новая установка",
  "installations.import": "Импорт",

  "skins.signinBody": "Войдите через аккаунт Microsoft, чтобы управлять скинами и плащами.",
  "skins.signinBtn": "Войти через Microsoft",
  "skins.variant.classic": "Классическая",
  "skins.variant.slim": "Тонкая",
  "skins.upload": "Загрузить скин…",
  "skins.reset": "Сбросить по умолчанию",
  "skins.capes": "Плащи",
  "skins.refresh": "Обновить",

  "notes.placeholder": "Здесь появятся патч-ноты.",

  "playbar.installing": "Установка…",
  "playbar.noInstances": "Нет установок",
  "playbar.play": "Играть",
  "playbar.notSignedIn": "Вы не вошли",

  "accounts.title": "Аккаунты",
  "accounts.back": "← Назад",
  "accounts.body": "Верхний аккаунт используется для запуска игры. Переместите аккаунт наверх, чтобы сделать его текущим.",
  "accounts.addOffline": "Добавить оффлайн",
  "accounts.addMicrosoft": "Добавить Microsoft",

  "settings.title": "Настройки",
  "settings.back": "← Назад",
  "settings.theme.label": "Тема",
  "settings.theme.hint": "Выберите цветовую схему.",
  "settings.theme.darkGreen": "Тёмная · Зелёная",
  "settings.theme.darkAmber": "Тёмная · Янтарная",
  "settings.theme.lightGreen": "Светлая · Зелёная",
  "settings.theme.lightAmber": "Светлая · Янтарная",
  "settings.theme.starlight": "Звёздный свет",
  "settings.language.label": "Язык",
  "settings.language.hint": "Язык интерфейса.",
  "settings.language.en": "English",
  "settings.language.ru": "Русский",
  "settings.snapshots.label": "Показывать снапшоты и старые версии",
  "settings.snapshots.hint": "Включить снапшоты, старые беты и альфы в список версий.",
  "settings.moments.label": "Вкладка «Моменты»",
  "settings.moments.hint": "Показывать вкладку «Моменты» со слайд-шоу скриншотов выбранной установки.",
  "settings.momentsBg.label": "Фон «Моментов»",
  "settings.momentsBg.hint": "Показывать скриншоты выбранной установки на фоне вкладки «Моменты».",
  "settings.blur.label": "Размытие фона",
  "settings.blur.hint": "Насколько размывать фон-скриншот на вкладке «Моменты».",
  "settings.gameData.label": "Данные игры",
  "settings.gameData.hint": "Загруженные версии, библиотеки и ресурсы. Общие для всех установок.",
  "settings.instances.label": "Установки",
  "settings.instances.hint": "Собственные сохранения, ресурспаки, шейдерпаки и конфигурация каждой установки.",
  "settings.curseforge.label": "API-ключ CurseForge",
  "settings.curseforge.hintPrefix":
    "Необязательно -- вставьте свой личный API-ключ CurseForge, чтобы включить просмотр модов, ресурспаков и шейдерпаков CurseForge (Beacon не может поставляться с общим ключом; условия CurseForge запрещают встраивать его в распространяемое приложение).",
  "settings.curseforge.notSet": "Не задан.",
  "settings.curseforge.set": "Ключ задан.",
  "settings.curseforge.placeholder": "Вставьте API-ключ",
  "settings.curseforge.save": "Сохранить",
  "settings.curseforge.clear": "Очистить",
  "settings.curseforge.request": "Получить ключ…",
  "settings.config.label": "Конфигурация лаунчера",
  "settings.config.hint": "Аккаунты, список установок и эти настройки. Отсюда не переносится.",
  "settings.wipe.label": "Стереть все данные",
  "settings.wipe.hint": "Удаляет все аккаунты, установки, миры и загруженные файлы, затем закрывает Beacon. Отменить нельзя.",
  "settings.wipe.button": "Стереть всё",
  "settings.open": "Открыть",
  "settings.browse": "Обзор…",
  "settings.moving": "Перемещение...",
  "settings.unknown": "Неизвестно",
  "toggle.on": "Вкл",
  "toggle.off": "Выкл",

  "instance.back": "← Назад",
  "instance.title": "Установка",
  "instance.changeIcon": "Изменить значок",
  "instance.start": "Запустить",
  "instance.stop": "Остановить",
  "instance.rename": "Переименовать",
  "instance.delete": "Удалить",
  "instance.moreActions": "Ещё действия",
  "instance.clearIcon": "Убрать значок",
  "instance.openFolder": "Открыть .minecraft",
  "instance.openLibraries": "Открыть библиотеки",
  "instance.export": "Экспорт",

  "instance.tab.overview": "Обзор",
  "instance.tab.mods": "Моды",
  "instance.tab.worlds": "Миры",
  "instance.tab.resourcePacks": "Ресурспаки",
  "instance.tab.shaderPacks": "Шейдерпаки",
  "instance.tab.screenshots": "Скриншоты",
  "instance.tab.advanced": "Дополнительно",

  "instance.version.title": "Версия",
  "instance.version.change": "Изменить версию",
  "instance.loader.installEllipsis": "Установить…",
  "instance.loader.changeEllipsis": "Изменить…",
  "instance.loader.remove": "Удалить",

  "instance.mods.title": "Моды",
  "instance.mods.openFolder": "Открыть папку",
  "instance.mods.browse": "Обзор модов…",
  "instance.mods.needLoader": "Сначала установите загрузчик модов",
  "instance.mods.delete": "Удалить",
  "instance.mods.add": "Добавить мод",
  "instance.mods.empty": "Модов пока нет.",
  "instance.worlds.title": "Миры",
  "instance.worlds.openFolder": "Открыть папку",
  "instance.worlds.empty": "Миров пока нет.",
  "instance.resourcePacks.title": "Ресурспаки",
  "instance.resourcePacks.openFolder": "Открыть папку",
  "instance.resourcePacks.browse": "Обзор ресурспаков…",
  "instance.resourcePacks.empty": "Ресурспаков пока нет.",
  "instance.shaderPacks.title": "Шейдерпаки",
  "instance.shaderPacks.openFolder": "Открыть папку",
  "instance.shaderPacks.browse": "Обзор шейдерпаков…",
  "instance.shaderPacks.empty": "Шейдерпаков пока нет.",
  "instance.screenshots.title": "Скриншоты",
  "instance.screenshots.openFolder": "Открыть папку",
  "instance.screenshots.empty": "Скриншотов пока нет.",
  "instance.advanced.log": "Журнал игры",
  "instance.advanced.clear": "Очистить",
  "instance.advanced.noSession": "Для этой установки нет активной сессии.",
  "instance.advanced.title": "Дополнительно",
  "instance.advanced.body": "Низкоуровневый патчинг jar-файла в стиле MultiMC. Пока не реализовано.",
  "instance.advanced.notImplemented": "Пока не реализовано",
  "instance.advanced.addModToJar": "Добавить мод в jar-файл Minecraft",
  "instance.advanced.replaceJar": "Заменить Minecraft.jar",
  "instance.advanced.addAgents": "Добавить агенты",
  "instance.advanced.addEmptyFile": "Добавить пустой файл",
  "instance.advanced.importComponents": "Импортировать компоненты",

  "login.title": "Вход через Microsoft",
  "login.body": "Откройте страницу ниже и введите этот код, чтобы завершить вход.",
  "login.openBrowser": "Открыть браузер",
  "login.close": "Закрыть",

  "error.title": "Неожиданная ошибка",
  "error.close": "Закрыть",

  "offline.title": "Добавить оффлайн-аккаунт",
  "offline.body": "Выберите никнейм. Он не обязан совпадать с вашим аккаунтом Microsoft.",
  "offline.placeholder": "Никнейм",
  "offline.add": "Добавить",
  "offline.cancel": "Отмена",

  "createInstance.title": "Новая установка",
  "createInstance.namePlaceholder": "Название установки",
  "createInstance.create": "Создать",
  "createInstance.cancel": "Отмена",

  "changeVersion.title": "Изменить версию",
  "changeVersion.cancel": "Отмена",

  "installLoader.title": "Установка загрузчика модов",
  "installLoader.installing": "Установка…",
  "installLoader.install": "Установить",
  "installLoader.cancel": "Отмена",

  "browseContent.back": "← Назад",
  "browseContent.modrinth": "Modrinth",
  "browseContent.curseforge": "CurseForge",
  "browseContent.curseforgeHint": "Вставьте API-ключ CurseForge в настройках, чтобы включить этот источник.",
  "browseContent.searchPlaceholder": "Поиск…",
  "browseContent.checkUpdates": "Проверить обновления",
  "browseContent.selectItem": "Выберите элемент, чтобы увидеть подробности.",
  "browseContent.reviewInstall": "Проверка и установка",
  "browseContent.reviewPlaceholder": "Отметьте элементы слева, чтобы просмотреть их здесь перед установкой.",
  "browseContent.installing": "Установка…",
  "browseContent.install": "Установить (0)",

  "renameInstance.title": "Переименовать установку",
  "renameInstance.confirm": "Переименовать",
  "renameInstance.cancel": "Отмена",

  "deleteInstance.title": "Удалить установку?",
  "deleteInstance.body": "Это безвозвратно удалит её миры и всё остальное содержимое папки. Отменить нельзя.",
  "deleteInstance.confirm": "Удалить",
  "deleteInstance.cancel": "Отмена",

  "wipe.title": "Стереть все данные?",
  "wipe.body": "Это безвозвратно удалит все аккаунты, установки, миры и загруженные файлы, затем закроет Beacon. Отменить нельзя. Если игра сейчас запущена, сначала закройте её.",
  "wipe.placeholder": "Введите WIPE для подтверждения",
  "wipe.confirm": "Стереть всё",
  "wipe.wiping": "Стирание...",
  "wipe.cancel": "Отмена",

  "play.play": "Играть",
  "play.signIn": "Войти",
  "play.newInstance": "Новая установка",
  "play.selectInstance": "Выберите установку",
  "play.installing": "Установка...",
  "play.checking": "Проверка...",
  "play.checkingFiles": "Проверка файлов...",
  "play.launching": "Запуск...",
  "play.running": "Запущено...",
  "play.downloading": "Загрузка",
  "play.checkingVerb": "Проверка",
  "play.launchFailedPrefix": "Не удалось запустить: {message}",

  "account.connecting": "Подключение...",
  "account.connectionFailed": "Не удалось подключиться. Войдите снова.",
  "account.connected": "Подключено",

  "instances.playbar.noInstances": "Нет установок",
  "instances.picker.selectInstance": "Выберите установку",
  "instances.picker.emptyList": "Установок пока нет.",
  "instances.grid.empty": "Установок пока нет. Создайте первую, чтобы начать.",
  "instances.loaderNone": "Загрузчик модов — нет",
  "instances.loaderNamed": "Загрузчик модов — {name}",
  "instances.versionPrefix": "Minecraft {version}",
  "instances.deleteBody": "Это безвозвратно удалит «{name}» -- её миры и всё остальное содержимое папки. Отменить нельзя.",

  "modContent.updateTo": "Обновить до {version}",
  "modContent.updating": "Обновление…",
  "modContent.removing": "Удаление…",
  "modContent.remove": "Удалить",
  "modContent.installedFmt": "Установить ({count})",
  "modContent.checking": "Проверка…",
  "modContent.startingInstall": "Запуск…",
  "modContent.bringsIn": "Включает: {list}",
  "modContent.removeConfirm": "Удалить {noun}?",
  "modContent.removeBody": "Это безвозвратно удалит «{filename}». Отменить нельзя.",
  "modContent.noun.mod": "мод",
  "modContent.noun.resourcePack": "ресурспак",
  "modContent.noun.shaderPack": "шейдерпак",
  "modContent.title.mod": "Обзор модов",
  "modContent.title.resourcePack": "Обзор ресурспаков",
  "modContent.title.shaderPack": "Обзор шейдерпаков",
  "modContent.placeholder.mod": "Поиск модов…",
  "modContent.placeholder.resourcePack": "Поиск ресурспаков…",
  "modContent.placeholder.shaderPack": "Поиск шейдерпаков…",
  "modContent.empty.mod": "Моды не найдены.",
  "modContent.empty.resourcePack": "Ресурспаки не найдены.",
  "modContent.empty.shaderPack": "Шейдерпаки не найдены.",
  "modContent.source.unknown": "Неизвестно",

  "common.remove": "Удалить",
  "common.unpin": "Открепить",
  "common.pinAsMomentsBg": "Закрепить как фон вкладки «Моменты»",
  "confirm.deleteFilePrefix": "Это безвозвратно удалит «{name}». Отменить нельзя.",
  "confirm.deleteWorld.title": "Удалить мир?",
  "confirm.deleteResourcePack.title": "Удалить ресурспак?",
  "confirm.deleteShaderPack.title": "Удалить шейдерпак?",
  "confirm.deleteMods.title": "Удалить моды?",
  "confirm.deleteModsBody.single": "Это безвозвратно удалит «{name}». Отменить нельзя.",
  "confirm.deleteModsBody.multi": "Это безвозвратно удалит {count} модов: {names}. Отменить нельзя.",
  "instance.mods.deleteFmt": "Удалить ({count})",
};

const dicts: Record<Lang, Dict> = { en, ru };

function detectDefaultLang(): Lang {
  return navigator.language.toLowerCase().startsWith("ru") ? "ru" : "en";
}

function readLang(): Lang {
  try {
    const stored = localStorage.getItem(LANG_KEY);
    return stored === "en" || stored === "ru" ? stored : detectDefaultLang();
  } catch {
    return "en";
  }
}

function writeLang(lang: Lang) {
  try {
    localStorage.setItem(LANG_KEY, lang);
  } catch {
    // Best-effort, same as every other localStorage-backed setting in this app.
  }
}

let currentLang: Lang = readLang();

export function getLang(): Lang {
  return currentLang;
}

export function setLang(lang: Lang) {
  if (lang === currentLang) return;
  currentLang = lang;
  writeLang(lang);
  applyI18n();
}

export function t(key: string, vars?: Record<string, string | number>): string {
  let str = dicts[currentLang][key] ?? en[key] ?? key;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) str = str.split(`{${name}}`).join(String(value));
  }
  return str;
}

// Walks every `data-i18n*`-tagged element under `root` and fills in the current language's copy
// -- called once at startup (`root` defaults to the whole document) and again, scoped to nothing
// in particular since it's cheap enough to just redo the whole document, whenever `setLang`
// switches languages.
export function applyI18n(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((node) => {
    const key = node.dataset.i18n;
    if (key) node.textContent = t(key);
  });
  root.querySelectorAll<HTMLInputElement>("[data-i18n-placeholder]").forEach((node) => {
    const key = node.dataset.i18nPlaceholder;
    if (key) node.placeholder = t(key);
  });
  root.querySelectorAll<HTMLElement>("[data-i18n-title]").forEach((node) => {
    const key = node.dataset.i18nTitle;
    if (key) node.title = t(key);
  });
  root.querySelectorAll<HTMLElement>("[data-i18n-aria-label]").forEach((node) => {
    const key = node.dataset.i18nAriaLabel;
    if (key) node.setAttribute("aria-label", t(key));
  });
  document.documentElement.lang = currentLang;
}
