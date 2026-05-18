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

const dashboardRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/",
  component: DashboardPage,
});
const postsListRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/posts",
  component: PostsListPage,
});
const postsDeletedRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/posts/deleted",
  component: PostsListPage,
});
const postEditRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/posts/$name",
  component: PostEditPage,
});
const postNewRoute = createRoute({
  getParentRoute: () => authenticatedRoute,
  path: "/posts/new",
  component: PostEditPage,
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
