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
} from "@ant-design/icons";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import dayjs from "dayjs";
import {
  listPosts,
  type PostSummary,
  softDeletePost,
  purgePost,
  restorePost,
  unpublishPost,
  publishPost,
} from "@/api/client";

const { Title } = Typography;
const { Search } = Input;

type PostAction =
  | "publish"
  | "unpublish"
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

export function PostsListPage() {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const isRecycleBin = pathname.endsWith("/posts/deleted");
  const { message } = App.useApp();
  const qc = useQueryClient();
  const [page, setPage] = useState(1);
  const [size, setSize] = useState(20);
  const [q, setQ] = useState("");
  const [status, setStatus] = useState<string | undefined>(undefined);
  const [visible, setVisible] = useState<string | undefined>(undefined);
  const [sort, setSort] = useState<string | undefined>(undefined);
  const [selected, setSelected] = useState<string[]>([]);

  const offset = (page - 1) * size;
  const query = useQuery({
    queryKey: ["posts", page, size, status, visible, isRecycleBin],
    queryFn: () =>
      listPosts({
        offset,
        limit: size,
        status,
        visible,
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

  const mutate = useMutation({
    mutationFn: async (action: { kind: PostAction; name: string }) => {
      if (action.kind === "publish") return publishPost(action.name);
      if (action.kind === "unpublish") return unpublishPost(action.name);
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
            : selected;
      for (const name of targets) {
        if (kind === "publish") await publishPost(name);
        if (kind === "unpublish") await unpublishPost(name);
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
        key: "edit",
        label: "Edit",
        icon: <EditOutlined />,
        onClick: () => navigate({ to: "/posts/$name", params: { name: post.name } }),
      },
      {
        key: "setting",
        label: "Setting",
        icon: <SettingOutlined />,
        onClick: () => navigate({ to: "/posts/$name", params: { name: post.name } }),
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
      <header className="halo-page-header">
        <div className="halo-page-header__title">
          {isRecycleBin ? <DeleteOutlined /> : <BookOutlined />}
          <Title level={3}>{isRecycleBin ? "Deleted Posts" : "Posts"}</Title>
        </div>
        <Space>
          {isRecycleBin ? (
            <Button size="small" onClick={() => navigate({ to: "/posts" })}>
              Back
            </Button>
          ) : (
            <>
              <Button size="small" onClick={() => navigate({ to: "/posts/deleted" })}>
                Recycle Bin
              </Button>
            </>
          )}
          <Button
            type="primary"
            icon={<PlusCircleOutlined />}
            onClick={() => navigate({ to: "/posts/new" })}
          >
            New
          </Button>
        </Space>
      </header>

      <Card className="halo-entity-card" bodyStyle={{ padding: 0 }}>
        <div className="halo-posts-toolbar">
          <Checkbox checked={allSelected} indeterminate={selected.length > 0 && !allSelected} onChange={(e) => setAllSelected(e.target.checked)} />
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
                  setPage(1);
                  setQ(event.target.value);
                }}
              />
            )}
          </div>
          {!isRecycleBin ? (
            <Space size="middle" className="halo-posts-filters">
              <Select
                size="small"
                value={status}
                placeholder="Status: All"
                allowClear
                style={{ width: 112 }}
                onChange={(value) => {
                  setPage(1);
                  setStatus(value);
                }}
                options={[
                  { value: "published", label: "Published" },
                  { value: "draft", label: "Draft" },
                  { value: "any", label: "All" },
                ]}
              />
              <Select
                size="small"
                value={visible}
                placeholder="Visible: All"
                allowClear
                style={{ width: 112 }}
                onChange={(value) => {
                  setPage(1);
                  setVisible(value);
                }}
                options={[
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
                onChange={setSort}
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

        {query.isLoading ? (
          <div className="halo-posts-loading"><Spin /></div>
        ) : posts.length ? (
          <div className="halo-entity-container">
            {posts.map((post) => (
              <Dropdown key={post.name} trigger={["contextMenu"]} menu={{ items: postMenu(post) }}>
                <article className={`halo-post-row${selected.includes(post.name) ? " halo-post-row--selected" : ""}`}>
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
                      onClick={() => navigate({ to: "/posts/$name", params: { name: post.name } })}
                    >
                      {post.title || post.slug}
                    </button>
                    <div className="halo-post-row__meta">
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
              setPage(nextPage);
              setSize(nextSize);
              setSelected([]);
            }}
          />
        </div>
      </Card>
    </div>
  );
}
