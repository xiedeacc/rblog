import { useMemo, useState, type ReactNode } from "react";
import { Avatar, Typography, App, Button, Modal, Input } from "antd";
import {
  DashboardOutlined,
  FileTextOutlined,
  MessageOutlined,
  UserOutlined,
  PictureOutlined,
  ApiOutlined,
  SettingOutlined,
  LogoutOutlined,
  SearchOutlined,
  FolderOutlined,
} from "@ant-design/icons";
import { Outlet, Link, useRouterState, useNavigate } from "@tanstack/react-router";
import { useAuthStore } from "@/state/auth";
import { logout } from "@/api/client";

const { Text } = Typography;

interface ConsoleMenuItem {
  key: string;
  label: string;
  icon: ReactNode;
}

interface ConsoleMenuGroup {
  key: string;
  label?: string;
  items: ConsoleMenuItem[];
}

const menuGroups: ConsoleMenuGroup[] = [
  {
    key: "dashboard",
    items: [{ key: "/", icon: <DashboardOutlined />, label: "Dashboard" }],
  },
  {
    key: "content",
    label: "Content",
    items: [
      { key: "/posts", icon: <FileTextOutlined />, label: "Posts" },
      { key: "/pages", icon: <FolderOutlined />, label: "Pages" },
      { key: "/comments", icon: <MessageOutlined />, label: "Comments" },
      { key: "/attachments", icon: <PictureOutlined />, label: "Attachments" },
    ],
  },
  {
    key: "system",
    label: "System",
    items: [
      { key: "/plugins", icon: <ApiOutlined />, label: "Plugins" },
      { key: "/users", icon: <UserOutlined />, label: "Users" },
      { key: "/settings", icon: <SettingOutlined />, label: "Settings" },
    ],
  },
];

const flatMenuItems = menuGroups.flatMap((group) => group.items);

function activeKey(pathname: string): string {
  const stripped = pathname.replace(/^\/admin/, "");
  if (stripped === "" || stripped === "/") return "/";
  const first = stripped.split("/").filter(Boolean)[0];
  const top = "/" + (first ?? "");
  return flatMenuItems.some((i) => i.key === top) ? top : "/";
}

function HaloLogo() {
  return (
    <span className="console-logo" aria-label="rblog">
      <span className="console-logo__text">rblog</span>
    </span>
  );
}

export function AppShell() {
  const { message } = App.useApp();
  const navigate = useNavigate();
  const user = useAuthStore((s) => s.user);
  const clear = useAuthStore((s) => s.clear);
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const [searchOpen, setSearchOpen] = useState(false);
  const [search, setSearch] = useState("");
  const selectedKey = activeKey(pathname);

  const filteredMenuItems = useMemo(() => {
    const normalized = search.trim().toLowerCase();
    if (!normalized) return flatMenuItems;
    return flatMenuItems.filter((item) => item.label.toLowerCase().includes(normalized));
  }, [search]);

  const onLogout = async () => {
    try {
      await logout();
    } catch {
      // swallow — we're clearing local state regardless
    }
    clear();
    void message.success("Signed out");
    navigate({ to: "/login" });
  };

  return (
    <div className="console-layout">
      <aside className="console-sidebar">
        <div className="console-sidebar__logo-container">
          <a
            className="console-sidebar__logo-link"
            href="/"
            target="_blank"
            rel="noreferrer"
            title="Visit homepage"
          >
            <HaloLogo />
          </a>
        </div>

        <div className="console-sidebar__content">
          <div className="console-sidebar__search-wrapper">
            <button
              type="button"
              className="console-sidebar__search"
              onClick={() => setSearchOpen(true)}
            >
              <SearchOutlined className="console-sidebar__search-icon" />
              <span className="console-sidebar__search-text">Search</span>
              <span className="console-sidebar__search-shortcut">
                {navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl"}+K
              </span>
            </button>
          </div>

          <nav className="console-menu" aria-label="Console navigation">
            {menuGroups.map((group) => (
              <div className="console-menu__group" key={group.key}>
                {group.label ? <div className="console-menu__label">{group.label}</div> : null}
                {group.items.map((item) => (
                  <Link
                    key={item.key}
                    to={item.key}
                    className={`console-menu__item${
                      selectedKey === item.key ? " console-menu__item--active" : ""
                    }`}
                  >
                    <span className="console-menu__icon">{item.icon}</span>
                    <span className="console-menu__title">{item.label}</span>
                  </Link>
                ))}
              </div>
            ))}
          </nav>
        </div>

        <div className="console-sidebar__profile">
          {user ? (
            <div className="console-user-profile">
              <Avatar className="console-user-profile__avatar" size={36} icon={<UserOutlined />} />
              <div className="console-user-profile__info">
                <div className="console-user-profile__identity">
                  <Text className="console-user-profile__name" ellipsis>
                    {user.display_name || user.name}
                  </Text>
                  <span className="console-user-profile__role">Administrator</span>
                </div>
                <div className="console-user-profile__meta">
                  <Button
                    className="console-user-profile__logout"
                    type="text"
                    size="small"
                    icon={<LogoutOutlined />}
                    onClick={onLogout}
                  >
                    Logout
                  </Button>
                </div>
              </div>
            </div>
          ) : (
            <Button size="small" onClick={() => navigate({ to: "/login" })}>
              Sign in
            </Button>
          )}
        </div>
      </aside>

      <main className="console-main">
        <div className="app-content">
          <Outlet />
        </div>
        <footer className="console-main__footer">
          <span className="console-main__footer-text">Powered by </span>
          <Link className="console-main__footer-link" to="/">
            Halo
          </Link>
        </footer>
      </main>

      <nav className="console-mobile-nav" aria-label="Console mobile navigation">
        {flatMenuItems.slice(0, 5).map((item) => (
          <Link
            key={item.key}
            to={item.key}
            className={`console-mobile-nav__item${
              selectedKey === item.key ? " console-mobile-nav__item--active" : ""
            }`}
            title={item.label}
          >
            {item.icon}
          </Link>
        ))}
        <button
          type="button"
          className="console-mobile-nav__item"
          onClick={() => setSearchOpen(true)}
          title="More"
        >
          <FolderOutlined />
        </button>
      </nav>

      <Modal
        title="Search"
        open={searchOpen}
        footer={null}
        onCancel={() => setSearchOpen(false)}
        destroyOnHidden
      >
        <Input
          autoFocus
          prefix={<SearchOutlined />}
          placeholder="Search console pages"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          onPressEnter={() => {
            const first = filteredMenuItems[0];
            if (!first) return;
            setSearchOpen(false);
            setSearch("");
            navigate({ to: first.key });
          }}
        />
        <div className="console-search-results">
          {filteredMenuItems.map((item) => (
            <button
              type="button"
              key={item.key}
              className="console-search-results__item"
              onClick={() => {
                setSearchOpen(false);
                setSearch("");
                navigate({ to: item.key });
              }}
            >
              <span className="console-search-results__icon">{item.icon}</span>
              <span>{item.label}</span>
            </button>
          ))}
          {!filteredMenuItems.length ? (
            <div className="console-search-results__empty">No pages found.</div>
          ) : null}
        </div>
      </Modal>
    </div>
  );
}
