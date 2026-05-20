import {
  createRouter,
  createRoute,
  createRootRoute,
  redirect,
  Outlet,
} from "@tanstack/react-router";
import { AppShell } from "@/components/AppShell";
import { LoginPage } from "@/pages/LoginPage";
import { BootstrapPage } from "@/pages/BootstrapPage";
import { DashboardPage } from "@/pages/DashboardPage";
import { PostsListPage } from "@/pages/PostsListPage";
import { PostEditPage } from "@/pages/PostEditPage";
import { PagesListPage } from "@/pages/PagesListPage";
import { PageEditPage } from "@/pages/PageEditPage";
import { CommentsPage } from "@/pages/CommentsPage";
import { UsersPage } from "@/pages/UsersPage";
import { AttachmentsPage } from "@/pages/AttachmentsPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { PluginsPage } from "@/pages/PluginsPage";
import { useAuthStore } from "@/state/auth";
import { fetchBootstrapStatus, fetchWhoAmI } from "@/api/client";

const rootRoute = createRootRoute({
  component: () => <Outlet />,
});

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  component: LoginPage,
});

const bootstrapRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/bootstrap",
  component: BootstrapPage,
});

const authenticatedRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "authenticated",
  beforeLoad: async () => {
    const auth = useAuthStore.getState();
    if (auth.user) return;
    try {
      const me = await fetchWhoAmI();
      useAuthStore.setState({ user: me });
    } catch {
      const status = await fetchBootstrapStatus();
      if (!status.bootstrapped) {
        throw redirect({ to: "/bootstrap" });
      }
      throw redirect({ to: "/login" });
    }
  },
  component: AppShell,
});

function numberSearch(value: unknown, fallback?: number): number | undefined {
  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function stringSearch(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value : undefined;
}

const validatePostsSearch = (search: Record<string, unknown>) => ({
  page: numberSearch(search.page, 1),
  size: numberSearch(search.size, 20),
  q: stringSearch(search.q),
  status: stringSearch(search.status),
  visible: stringSearch(search.visible),
  sort: stringSearch(search.sort),
  source: stringSearch(search.source),
});

const dashboardRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/",
  component: DashboardPage,
});
const postsListRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/posts",
  validateSearch: validatePostsSearch,
  component: PostsListPage,
});
const postsDeletedRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/posts/deleted",
  validateSearch: validatePostsSearch,
  component: PostsListPage,
});
const postEditRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/posts/$name",
  validateSearch: validatePostsSearch,
  component: PostEditPage,
});
const postNewRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/posts/new",
  validateSearch: validatePostsSearch,
  component: PostEditPage,
});
const pagesListRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/pages",
  validateSearch: validatePostsSearch,
  component: PagesListPage,
});
const pageEditRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/pages/$name",
  validateSearch: validatePostsSearch,
  component: PageEditPage,
});
const commentsRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/comments",
  component: CommentsPage,
});
const usersRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/users",
  component: UsersPage,
});
const attachmentsRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/attachments",
  component: AttachmentsPage,
});
const settingsRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/settings",
  component: SettingsPage,
});
const pluginsRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/plugins",
  component: PluginsPage,
});

const routeTree = rootRoute.addChildren([
  loginRoute,
  bootstrapRoute,
  authenticatedRoute.addChildren([
    dashboardRoute,
    postsListRoute,
    postsDeletedRoute,
    postNewRoute,
    postEditRoute,
    pagesListRoute,
    pageEditRoute,
    commentsRoute,
    usersRoute,
    attachmentsRoute,
    pluginsRoute,
    settingsRoute,
  ]),
]);

export const router = createRouter({
  routeTree,
  // The server mounts the SPA under /admin, so React Router roots there.
  basepath: "/admin",
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
