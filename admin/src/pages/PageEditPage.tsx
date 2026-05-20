import { useEffect, useState } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  Drawer,
  Form,
  Input,
  InputNumber,
  Select,
  Space,
  Spin,
  Switch,
  Typography,
} from "antd";
import { ArrowLeftOutlined, SaveOutlined, SendOutlined, SettingOutlined } from "@ant-design/icons";
import { useNavigate, useParams, useSearch } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  fetchPage,
  publishPage,
  unpublishPage,
  updatePageContent,
} from "@/api/client";
import { MarkdownEditor } from "@/components/MarkdownEditor";

const { Text, Title } = Typography;

interface FormValues {
  title: string;
  slug?: string;
  excerpt?: string;
  visible?: string;
  cover?: string;
  template?: string;
  priority?: number;
  pinned?: boolean;
  allow_comment?: boolean;
  publish_time?: string | null;
}

interface PagesSearch {
  page: number | undefined;
  size: number | undefined;
  q: string | undefined;
  status: string | undefined;
  visible: string | undefined;
  sort: string | undefined;
  source: string | undefined;
}

function compactPagesSearch(search: PagesSearch): PagesSearch {
  return {
    page: search.page === 1 ? undefined : search.page,
    size: search.size === 20 ? undefined : search.size,
    q: search.q || undefined,
    status: search.status,
    visible: search.visible,
    sort: undefined,
    source: undefined,
  };
}

export function PageEditPage() {
  const params = useParams({ strict: false }) as { name?: string };
  const name = params.name ?? "";
  const listSearch = compactPagesSearch(useSearch({ strict: false }) as PagesSearch);
  const navigate = useNavigate();
  const { message } = App.useApp();
  const qc = useQueryClient();
  const [form] = Form.useForm<FormValues>();
  const [markdown, setMarkdown] = useState("");
  const [published, setPublished] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [dirty, setDirty] = useState(false);

  const page = useQuery({
    queryKey: ["page", name],
    queryFn: () => fetchPage(name),
    enabled: Boolean(name),
  });

  useEffect(() => {
    if (page.data) {
      form.setFieldsValue({
        title: page.data.title,
        slug: page.data.slug,
        excerpt: page.data.excerpt,
        visible: page.data.visible,
        cover: page.data.cover ?? "",
        template: page.data.template ?? "",
        priority: page.data.priority,
        pinned: page.data.pinned,
        allow_comment: page.data.allow_comment,
        publish_time: page.data.publish_time,
      });
      setMarkdown(page.data.raw_markdown ?? "");
      setPublished(page.data.published);
      setDirty(false);
    }
  }, [form, page.data]);

  const save = useMutation({
    mutationFn: async () => {
      const values = await form.validateFields();
      return updatePageContent(name, {
        markdown,
        title: values.title,
        slug: values.slug,
        excerpt: values.excerpt,
        visible: values.visible,
        cover: values.cover,
        template: values.template,
        priority: values.priority,
        pinned: values.pinned,
        allow_comment: values.allow_comment,
        publish_time: values.publish_time || null,
      });
    },
    onSuccess: (detail) => {
      setDirty(false);
      setPublished(detail.published);
      void message.success("Saved");
      void qc.invalidateQueries({ queryKey: ["pages"] });
      void qc.invalidateQueries({ queryKey: ["page", name] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Save failed"),
  });

  const publish = useMutation({
    mutationFn: async () => (published ? unpublishPage(name) : publishPage(name)),
    onSuccess: (detail) => {
      setPublished(detail.published);
      void message.success(detail.published ? "Published" : "Unpublished");
      void qc.invalidateQueries({ queryKey: ["pages"] });
      void qc.invalidateQueries({ queryKey: ["page", name] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });

  if (page.isLoading) {
    return (
      <Card>
        <Spin />
      </Card>
    );
  }

  if (page.isError) {
    return (
      <Card>
        <Alert
          type="error"
          showIcon
          message="Failed to load page"
          description={page.error instanceof Error ? page.error.message : "Unable to fetch this page."}
        />
      </Card>
    );
  }

  const rawType = (page.data?.raw_type ?? "markdown").toLowerCase();
  const backToPages = () => {
    void navigate({ to: "/pages", search: listSearch });
  };

  return (
    <Card>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <Title level={3} style={{ margin: 0 }}>
            {page.data?.title || "Edit page"}
          </Title>
          <Space>
            <Button icon={<ArrowLeftOutlined />} onClick={backToPages}>
              Back
            </Button>
            <Button icon={<SettingOutlined />} onClick={() => setSettingsOpen(true)}>
              Setting
            </Button>
            <Button
              icon={<SaveOutlined />}
              type="primary"
              loading={save.isPending}
              onClick={() => save.mutate()}
            >
              Save
            </Button>
            <Button icon={<SendOutlined />} loading={publish.isPending} onClick={() => publish.mutate()}>
              {published ? "Unpublish" : "Publish"}
            </Button>
          </Space>
        </div>

        {dirty ? <Text type="secondary">Unsaved changes</Text> : null}

        {rawType !== "markdown" ? (
          <Alert
            type="info"
            showIcon
            message={`This page was imported as ${rawType.toUpperCase()}`}
            description="The editor shows the original source format on the left. Save will persist the current body as Markdown."
          />
        ) : null}

        <Form form={form} layout="vertical" onValuesChange={() => setDirty(true)}>
          <Form.Item name="title" label="Title" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <MarkdownEditor
            initialMarkdown={markdown}
            onChange={(next) => {
              setMarkdown(next);
              setDirty(true);
            }}
          />
        </Form>
      </Space>

      <Drawer
        title="Page settings"
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        width={420}
      >
        <Form form={form} layout="vertical" onValuesChange={() => setDirty(true)}>
          <Form.Item name="slug" label="Slug" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="excerpt" label="Excerpt">
            <Input.TextArea rows={4} />
          </Form.Item>
          <Form.Item name="visible" label="Visible" initialValue="PUBLIC">
            <Select
              options={[
                { value: "PUBLIC", label: "Public" },
                { value: "PRIVATE", label: "Private" },
                { value: "INTERNAL", label: "Internal" },
              ]}
            />
          </Form.Item>
          <Form.Item name="cover" label="Cover">
            <Input />
          </Form.Item>
          <Form.Item name="template" label="Template">
            <Input />
          </Form.Item>
          <Form.Item name="priority" label="Priority">
            <InputNumber style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="pinned" label="Pinned" valuePropName="checked">
            <Switch />
          </Form.Item>
          <Form.Item name="allow_comment" label="Allow comments" valuePropName="checked">
            <Switch />
          </Form.Item>
          <Form.Item name="publish_time" label="Publish time">
            <Input placeholder="ISO datetime, leave blank for default" />
          </Form.Item>
        </Form>
      </Drawer>
    </Card>
  );
}
