import { useMemo, useState } from "react";
import {
  Button,
  Card,
  Checkbox,
  Dropdown,
  Empty,
  Input,
  Pagination,
  Popconfirm,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
  App,
  type MenuProps,
} from "antd";
import {
  DeleteOutlined,
  EditOutlined,
  PlusCircleOutlined,
  ReloadOutlined,
  SettingOutlined,
  RollbackOutlined,
  BookOutlined,
  PushpinOutlined,
} from "@ant-design/icons";
import { useNavigate, useRouterState, useSearch } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import dayjs from "dayjs";
import {
  createPost,
  listPosts,
  type PostSummary,
  softDeletePost,
  purgePost,
  restorePost,
  unpublishPost,
  publishPost,
  pinPost,
  unpinPost,
} from "@/api/client";

const { Title } = Typography;
const { Search } = Input;

interface PostsSearch {
  page: number | undefined;
  size: number | undefined;
  q: string | undefined;
  status: string | undefined;
  visible: string | undefined;
  sort: string | undefined;
  source: string | undefined;
  returnTo: string | undefined;
}

type PostAction =
  | "publish"
  | "unpublish"
  | "pin"
  | "unpin"
  | "delete"
  | "purge"
  | "restore";

function formatDate(value: string | null): string {
  return value ? dayjs(value).format("YYYY-MM-DD HH:mm") : "-";
}

function visibilityLabel(value: string): string {
  if (value === "PRIVATE") return "Private";
  if (value === "INTERNAL") return "Internal";
  return "Public";
}

function normalizeStatus(value: string | undefined): "any" | "published" | "draft" {
  if (value === "published" || value === "draft") return value;
  return "any";
}

function buildTradingCalendarPost(now = dayjs()) {
  const cycleStart = now.day() === 0 ? now.startOf("day") : now.subtract(now.day(), "day").startOf("day");
  const monday = cycleStart.add(1, "day");
  const friday = cycleStart.add(5, "day");
  const weekdays = Array.from({ length: 5 }, (_, index) => monday.add(index, "day"));
  const start = monday.format("YYYYMMDD");
  const end = friday.format("YYYYMMDD");
  const slug = `trading-calendar-${start}-${end}`;

  return {
    name: slug,
    title: `交易日历——${start}-${end}`,
    slug,
    markdown: weekdays.map((day) => `## ${day.format("YYYYMMDD")}`).join("\n\n\n\n"),
  };
}

export function PostsListPage() {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const routeSearch = useSearch({ strict: false }) as PostsSearch;
  const isRecycleBin = pathname.endsWith("/posts/deleted");
  const { message } = App.useApp();
  const qc = useQueryClient();
  const [selected, setSelected] = useState<string[]>([]);
  const page = routeSearch.page ?? 1;
  const size = routeSearch.size ?? 20;
  const q = routeSearch.q ?? "";
  const status = normalizeStatus(routeSearch.status);
  const apiStatus = status === "any" ? undefined : status;
  const visible = isRecycleBin ? routeSearch.visible : (routeSearch.visible ?? "PUBLIC");
  const apiVisible = visible === "any" ? undefined : visible;
  const sort = routeSearch.sort;
  const listSearch = useMemo(
    () => ({
      page: page === 1 ? undefined : page,
      size: size === 20 ? undefined : size,
      q: q || undefined,
      status: status === "any" ? undefined : status,
      visible: visible === "PUBLIC" ? undefined : visible,
      sort,
      source: undefined,
      returnTo: undefined,
    }),
    [page, q, size, sort, status, visible],
  );
  const updateSearch = (patch: Partial<PostsSearch>) => {
    setSelected([]);
    void navigate({
      to: isRecycleBin ? "/posts/deleted" : "/posts",
      replace: true,
      search: {
        ...listSearch,
        ...patch,
      },
    });
  };
  const openPost = (postName: string) => {
    void navigate({
      to: "/posts/$name",
      params: { name: postName },
      search: { ...listSearch, source: isRecycleBin ? "deleted" : undefined },
    });
  };

  const offset = (page - 1) * size;
  const query = useQuery({
    queryKey: ["posts", page, size, status, apiVisible, isRecycleBin],
    queryFn: () =>
      listPosts({
        offset,
        limit: size,
        status: apiStatus,
        visible: apiVisible,
        include_deleted: isRecycleBin,
        deleted_only: isRecycleBin,
      }),
  });

  const posts = useMemo(() => {
    const normalized = q.trim().toLowerCase();
    const items = [...(query.data?.items ?? [])].filter((post) => {
      if (!normalized) return true;
      return (
        post.title.toLowerCase().includes(normalized) ||
        post.slug.toLowerCase().includes(normalized) ||
        post.name.toLowerCase().includes(normalized)
      );
    });
    if (sort === "last_modify_time,asc") {
      items.sort((a, b) => (a.last_modify_time ?? "").localeCompare(b.last_modify_time ?? ""));
    } else if (sort === "last_modify_time,desc") {
      items.sort((a, b) => (b.last_modify_time ?? "").localeCompare(a.last_modify_time ?? ""));
    } else if (sort === "publish_time,asc") {
      items.sort((a, b) => (a.publish_time ?? "").localeCompare(b.publish_time ?? ""));
    } else if (sort === "publish_time,desc") {
      items.sort((a, b) => (b.publish_time ?? "").localeCompare(a.publish_time ?? ""));
    }
    return items;
  }, [q, query.data?.items, sort]);

  const allSelected = posts.length > 0 && posts.every((post) => selected.includes(post.name));
  const selectedPosts = posts.filter((post) => selected.includes(post.name));
  const selectedDrafts = selectedPosts.filter((post) => !post.published);
  const selectedPublished = selectedPosts.filter((post) => post.published);
  const selectedUnpinned = selectedPosts.filter((post) => !post.pinned);
  const selectedPinned = selectedPosts.filter((post) => post.pinned);

  const mutate = useMutation({
    mutationFn: async (action: { kind: PostAction; name: string }) => {
      if (action.kind === "publish") return publishPost(action.name);
      if (action.kind === "unpublish") return unpublishPost(action.name);
      if (action.kind === "pin") return pinPost(action.name);
      if (action.kind === "unpin") return unpinPost(action.name);
      if (action.kind === "purge") return purgePost(action.name);
      if (action.kind === "restore") return restorePost(action.name);
      return softDeletePost(action.name);
    },
    onSuccess: () => {
      void message.success("Done");
      void qc.invalidateQueries({ queryKey: ["posts"] });
      setSelected([]);
    },
    onError: (err) => {
      void message.error(err instanceof Error ? err.message : "Action failed");
    },
  });

  const batchMutate = useMutation({
    mutationFn: async (kind: PostAction) => {
      const targets =
        kind === "publish"
          ? selectedDrafts.map((post) => post.name)
          : kind === "unpublish"
            ? selectedPublished.map((post) => post.name)
            : kind === "pin"
              ? selectedUnpinned.map((post) => post.name)
              : kind === "unpin"
                ? selectedPinned.map((post) => post.name)
                : selected;
      for (const name of targets) {
        if (kind === "publish") await publishPost(name);
        if (kind === "unpublish") await unpublishPost(name);
        if (kind === "pin") await pinPost(name);
        if (kind === "unpin") await unpinPost(name);
        if (kind === "delete") await softDeletePost(name);
        if (kind === "purge") await purgePost(name);
        if (kind === "restore") await restorePost(name);
      }
    },
    onSuccess: () => {
      void message.success("Done");
      setSelected([]);
      void qc.invalidateQueries({ queryKey: ["posts"] });
    },
    onError: (err) => void message.error(err instanceof Error ? err.message : "Action failed"),
  });

  const createTradingPost = useMutation({
    mutationFn: async () => createPost({ ...buildTradingCalendarPost(), visible: "PUBLIC" }),
    onSuccess: (detail) => {
      void message.success("Draft created");
      void qc.invalidateQueries({ queryKey: ["posts"] });
      void navigate({
        to: "/posts/$name",
        params: { name: detail.name },
        search: listSearch,
      });
    },
    onError: (err) => {
      void message.error(err instanceof Error ? err.message : "Create failed");
    },
  });

  const toggleSelected = (name: string, checked: boolean) => {
    setSelected((items) => (checked ? [...items, name] : items.filter((item) => item !== name)));
  };

  const setAllSelected = (checked: boolean) => {
    setSelected(checked ? posts.map((post) => post.name) : []);
  };

  const postMenu = (post: PostSummary): MenuProps["items"] => {
    if (isRecycleBin) {
      return [
        {
          key: "restore",
          label: "Restore",
          icon: <RollbackOutlined />,
          onClick: () => mutate.mutate({ kind: "restore", name: post.name }),
        },
        {
          key: "purge",
          label: "Delete permanently",
          icon: <DeleteOutlined />,
          danger: true,
          onClick: () => mutate.mutate({ kind: "purge", name: post.name }),
        },
      ];
    }
    return [
      {
        key: "publish",
        label: "Publish",
        icon: <BookOutlined />,
        disabled: post.published,
        onClick: () => mutate.mutate({ kind: "publish", name: post.name }),
      },
      {
        key: post.pinned ? "unpin" : "pin",
        label: post.pinned ? "Cancel Pin" : "Pin",
        icon: <PushpinOutlined />,
        onClick: () => mutate.mutate({ kind: post.pinned ? "unpin" : "pin", name: post.name }),
      },
      {
        key: "edit",
        label: "Edit",
        icon: <EditOutlined />,
        onClick: () => openPost(post.name),
      },
      {
        key: "setting",
        label: "Setting",
        icon: <SettingOutlined />,
        onClick: () => openPost(post.name),
      },
      { type: "divider" },
      {
        key: "unpublish",
        label: "Cancel Publish",
        danger: true,
        disabled: !post.published,
        onClick: () => mutate.mutate({ kind: "unpublish", name: post.name }),
      },
      {
        key: "delete",
        label: "Delete",
        icon: <DeleteOutlined />,
        danger: true,
        onClick: () => mutate.mutate({ kind: "delete", name: post.name }),
      },
    ];
  };

  return (
    <div className="halo-posts-page">
      <div className="halo-posts-sticky-top">
        <header className="halo-page-header">
          <div className="halo-page-header__title">
            {isRecycleBin ? <DeleteOutlined /> : <BookOutlined />}
            <Title level={3}>{isRecycleBin ? "Deleted Posts" : "Posts"}</Title>
          </div>
          <Space>
            {isRecycleBin ? (
              <Button size="small" onClick={() => navigate({ to: "/posts", search: listSearch })}>
                Back
              </Button>
            ) : (
              <>
                <Button size="small" onClick={() => navigate({ to: "/posts/deleted", search: listSearch })}>
                  Recycle Bin
                </Button>
              </>
            )}
            <Button
              type="primary"
              icon={<PlusCircleOutlined />}
              onClick={() => navigate({ to: "/posts/new", search: listSearch })}
            >
              New
            </Button>
            <Button
              type="primary"
              icon={<PlusCircleOutlined />}
              loading={createTradingPost.isPending}
              onClick={() => createTradingPost.mutate()}
            >
              New交易博客
            </Button>
          </Space>
        </header>
        <div className="halo-posts-toolbar">
          <div className="halo-posts-toolbar__checkbox">
            <Checkbox checked={allSelected} indeterminate={selected.length > 0 && !allSelected} onChange={(e) => setAllSelected(e.target.checked)} />
          </div>
          <div className="halo-posts-toolbar__main">
            {selected.length ? (
              <Space>
                {isRecycleBin ? (
                  <>
                    <Popconfirm title="Delete selected posts permanently?" onConfirm={() => batchMutate.mutate("purge")}>
                      <Button danger>Delete permanently</Button>
                    </Popconfirm>
                    <Button onClick={() => batchMutate.mutate("restore")}>Restore</Button>
                  </>
                ) : (
                  <>
                    {selectedDrafts.length ? (
                      <Button onClick={() => batchMutate.mutate("publish")}>Publish</Button>
                    ) : null}
                    {selectedPublished.length ? (
                      <Button onClick={() => batchMutate.mutate("unpublish")}>Cancel Publish</Button>
                    ) : null}
                    {selectedUnpinned.length ? (
                      <Button icon={<PushpinOutlined />} onClick={() => batchMutate.mutate("pin")}>Pin</Button>
                    ) : null}
                    {selectedPinned.length ? (
                      <Button icon={<PushpinOutlined />} onClick={() => batchMutate.mutate("unpin")}>Cancel Pin</Button>
                    ) : null}
                    <Popconfirm title="Move selected posts to recycle bin?" onConfirm={() => batchMutate.mutate("delete")}>
                      <Button danger>Delete</Button>
                    </Popconfirm>
                  </>
                )}
              </Space>
            ) : (
              <Search
                allowClear
                placeholder="Search"
                value={q}
                onChange={(event) => {
                  updateSearch({ page: undefined, q: event.target.value || undefined });
                }}
              />
            )}
          </div>
          {!isRecycleBin ? (
            <Space size="middle" className="halo-posts-filters">
              <Select
                size="small"
                value={status}
                style={{ width: 112 }}
                onChange={(value) => {
                  updateSearch({ page: undefined, status: value === "any" ? undefined : value });
                }}
                options={[
                  { value: "any", label: "All" },
                  { value: "published", label: "Published" },
                  { value: "draft", label: "Draft" },
                ]}
              />
              <Select
                size="small"
                value={visible}
                placeholder="Visible: All"
                allowClear
                style={{ width: 112 }}
                onChange={(value) => {
                  updateSearch({ page: undefined, visible: value });
                }}
                options={[
                  { value: "any", label: "All" },
                  { value: "PUBLIC", label: "Public" },
                  { value: "PRIVATE", label: "Private" },
                  { value: "INTERNAL", label: "Internal" },
                ]}
              />
              <Select
                size="small"
                value={sort}
                placeholder="Sort: Default"
                allowClear
                style={{ width: 124 }}
                onChange={(value) => updateSearch({ sort: value })}
                options={[
                  { value: "publish_time,desc", label: "Publish ↓" },
                  { value: "publish_time,asc", label: "Publish ↑" },
                  { value: "last_modify_time,desc", label: "Updated ↓" },
                  { value: "last_modify_time,asc", label: "Updated ↑" },
                ]}
              />
            </Space>
          ) : null}
          <Button
            type="text"
            size="small"
            icon={<ReloadOutlined spin={query.isFetching} />}
            onClick={() => query.refetch()}
          />
        </div>
      </div>

      <Card className="halo-entity-card" bodyStyle={{ padding: 0 }}>
        {query.isLoading ? (
          <div className="halo-posts-loading"><Spin /></div>
        ) : posts.length ? (
          <div className="halo-entity-container">
            {posts.map((post) => (
              <Dropdown key={post.name} trigger={["contextMenu"]} menu={{ items: postMenu(post) }}>
                <article className={`halo-post-row${selected.includes(post.name) ? " halo-post-row--selected" : ""}${post.pinned ? " halo-post-row--pinned" : ""}`}>
                  <div className="halo-post-row__checkbox">
                    <Checkbox
                      checked={selected.includes(post.name)}
                      onChange={(event) => toggleSelected(post.name, event.target.checked)}
                    />
                  </div>
                  <div className="halo-post-row__main">
                    <button
                      type="button"
                      className="halo-post-row__title"
                      onClick={() => openPost(post.name)}
                    >
                      {post.pinned ? "[置顶] " : ""}{post.title || post.slug}
                    </button>
                    <div className="halo-post-row__meta">
                      {post.pinned ? (
                        <Tag color="gold" icon={<PushpinOutlined />}>Pinned</Tag>
                      ) : null}
                      <span>{post.visits} Visits</span>
                      <span>{post.comments_count} Comments</span>
                    </div>
                  </div>
                  <div className="halo-post-row__fields">
                    {!isRecycleBin ? (
                      <>
                        <span className={`halo-post-status halo-post-status--${post.published ? "published" : "draft"}`}>
                          {post.published ? "Published" : "Draft"}
                        </span>
                        <span className="halo-post-field">{visibilityLabel(post.visible)}</span>
                        <span className="halo-post-field" title={formatDate(post.last_modify_time)}>
                          {formatDate(post.last_modify_time)}
                        </span>
                      </>
                    ) : (
                      <>
                        <span className="halo-post-status halo-post-status--deleted">Deleted</span>
                        <span className="halo-post-field" title={formatDate(post.deletion_time)}>
                          {formatDate(post.deletion_time)}
                        </span>
                      </>
                    )}
                  </div>
                </article>
              </Dropdown>
            ))}
          </div>
        ) : (
          <Empty className="halo-posts-empty" description={isRecycleBin ? "Recycle bin is empty" : "No posts"} />
        )}

        <div className="halo-posts-pagination">
          <Pagination
            current={page}
            pageSize={size}
            total={query.data?.total ?? 0}
            pageSizeOptions={[20, 30, 50, 100]}
            showSizeChanger
            showTotal={(total) => `${total} items`}
            onChange={(nextPage, nextSize) => {
              updateSearch({
                page: nextPage === 1 ? undefined : nextPage,
                size: nextSize === 20 ? undefined : nextSize,
              });
            }}
          />
        </div>
      </Card>
    </div>
  );
}
