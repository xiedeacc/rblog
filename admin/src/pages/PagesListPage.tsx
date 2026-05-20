import { useMemo } from "react";
import {
  Button,
  Card,
  Empty,
  Input,
  Pagination,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
} from "antd";
import {
  EditOutlined,
  FileTextOutlined,
  PictureOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import dayjs from "dayjs";
import { listPages } from "@/api/client";

const { Title } = Typography;
const { Search } = Input;

interface PagesSearch {
  page: number | undefined;
  size: number | undefined;
  q: string | undefined;
  status: string | undefined;
  visible: string | undefined;
  sort: string | undefined;
  source: string | undefined;
  returnTo: string | undefined;
}

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

export function PagesListPage() {
  const navigate = useNavigate();
  const routeSearch = useSearch({ strict: false }) as PagesSearch;
  const page = routeSearch.page ?? 1;
  const size = routeSearch.size ?? 20;
  const q = routeSearch.q ?? "";
  const status = normalizeStatus(routeSearch.status);
  const visible = routeSearch.visible ?? "any";
  const apiStatus = status === "any" ? undefined : status;
  const apiVisible = visible === "any" ? undefined : visible;
  const listSearch = useMemo(
    () => ({
      page: page === 1 ? undefined : page,
      size: size === 20 ? undefined : size,
      q: q || undefined,
      status: status === "any" ? undefined : status,
      visible: visible === "any" ? undefined : visible,
      sort: undefined,
      source: undefined,
      returnTo: undefined,
    }),
    [page, q, size, status, visible],
  );
  const updateSearch = (patch: Partial<PagesSearch>) => {
    void navigate({
      to: "/pages",
      replace: true,
      search: {
        ...listSearch,
        ...patch,
      },
    });
  };
  const offset = (page - 1) * size;
  const query = useQuery({
    queryKey: ["pages", page, size, status, apiVisible],
    queryFn: () =>
      listPages({
        offset,
        limit: size,
        status: apiStatus,
        visible: apiVisible,
      }),
  });

  const pages = useMemo(() => {
    const normalized = q.trim().toLowerCase();
    return [...(query.data?.items ?? [])].filter((item) => {
      if (!normalized) return true;
      return (
        item.title.toLowerCase().includes(normalized) ||
        item.slug.toLowerCase().includes(normalized) ||
        item.name.toLowerCase().includes(normalized)
      );
    });
  }, [q, query.data?.items]);

  return (
    <div className="halo-posts-page">
      <div className="halo-posts-sticky-top">
        <header className="halo-page-header">
          <div className="halo-page-header__title">
            <FileTextOutlined />
            <Title level={3}>Pages</Title>
          </div>
        </header>
        <div className="halo-posts-toolbar">
          <div className="halo-posts-toolbar__main">
            <Search
              allowClear
              placeholder="Search"
              value={q}
              onChange={(event) => updateSearch({ page: undefined, q: event.target.value || undefined })}
            />
          </div>
          <Space size="middle" className="halo-posts-filters">
            <Select
              size="small"
              value={status}
              style={{ width: 112 }}
              onChange={(value) => updateSearch({ page: undefined, status: value === "any" ? undefined : value })}
              options={[
                { value: "any", label: "All" },
                { value: "published", label: "Published" },
                { value: "draft", label: "Draft" },
              ]}
            />
            <Select
              size="small"
              value={visible}
              style={{ width: 112 }}
              onChange={(value) => updateSearch({ page: undefined, visible: value === "any" ? undefined : value })}
              options={[
                { value: "any", label: "All" },
                { value: "PUBLIC", label: "Public" },
                { value: "PRIVATE", label: "Private" },
                { value: "INTERNAL", label: "Internal" },
              ]}
            />
          </Space>
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
        ) : pages.length ? (
          <div className="halo-entity-container">
            {pages.map((item) => (
              <article key={item.name} className="halo-post-row halo-page-row">
                <div className="halo-post-row__main">
                  <button
                    type="button"
                    className="halo-post-row__title"
                    onClick={() => navigate({ to: "/pages/$name", params: { name: item.name }, search: listSearch })}
                  >
                    {item.title || item.slug}
                  </button>
                  <div className="halo-post-row__meta">
                    <span>{item.visits} Visits</span>
                    <span>{item.comments_count} Comments</span>
                    <span><PictureOutlined /> {item.image_count} Images</span>
                    <Tag>{item.slug}</Tag>
                  </div>
                </div>
                <div className="halo-post-row__fields">
                  <span className={`halo-post-status halo-post-status--${item.published ? "published" : "draft"}`}>
                    {item.published ? "Published" : "Draft"}
                  </span>
                  <span className="halo-post-field">{visibilityLabel(item.visible)}</span>
                  <span className="halo-post-field" title={formatDate(item.last_modify_time)}>
                    {formatDate(item.last_modify_time)}
                  </span>
                  <Button
                    size="small"
                    type="text"
                    icon={<EditOutlined />}
                    onClick={() => navigate({ to: "/pages/$name", params: { name: item.name }, search: listSearch })}
                  />
                </div>
              </article>
            ))}
          </div>
        ) : (
          <Empty className="halo-posts-empty" description="No pages" />
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
