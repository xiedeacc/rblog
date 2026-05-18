import { useEffect, useState } from "react";
import {
  Button,
  Card,
  Form,
  Input,
  Select,
  Space,
  Typography,
  App,
  Drawer,
  Switch,
  InputNumber,
} from "antd";
import { SaveOutlined, SendOutlined, SettingOutlined } from "@ant-design/icons";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  fetchPost,
  createPost,
  updatePostContent,
  publishPost,
  unpublishPost,
} from "@/api/client";
import { MarkdownEditor } from "@/components/MarkdownEditor";

const { Title } = Typography;

interface FormValues {
  name?: string;
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

function slugify(text: string): string {
  return text
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^\w\s-]/g, "")
    .trim()
    .replace(/\s+/g, "-")
    .slice(0, 64);
}

export function PostEditPage() {
  const params = useParams({ strict: false }) as { name?: string };
  const isNew = !params.name;
  const name = params.name ?? "";
  const navigate = useNavigate();
  const { message } = App.useApp();
  const qc = useQueryClient();

  const [form] = Form.useForm<FormValues>();
  const [markdown, setMarkdown] = useState("");
  const [published, setPublished] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const post = useQuery({
    queryKey: ["post", name],
    queryFn: () => fetchPost(name),
    enabled: !isNew,
  });
  useEffect(() => {
    if (post.data) {
      form.setFieldsValue({
        name: post.data.name,
        title: post.data.title,
        slug: post.data.slug,
        excerpt: post.data.excerpt,
        visible: post.data.visible,
        cover: post.data.cover ?? "",
        template: post.data.template ?? "",
        priority: post.data.priority,
        pinned: post.data.pinned,
        allow_comment: post.data.allow_comment,
        publish_time: post.data.publish_time,
      });
      setMarkdown(post.data.raw_markdown ?? "");
      setPublished(post.data.published);
    }
  }, [post.data, form]);

  const save = useMutation({
    mutationFn: async () => {
      const values = await form.validateFields();
      if (isNew) {
        const slug = values.slug || slugify(values.title);
        const generatedName = values.name || `post-${Date.now().toString(36)}`;
        return createPost({
          name: generatedName,
          title: values.title,
          slug,
          markdown,
          visible: values.visible,
          cover: values.cover,
          template: values.template,
          priority: values.priority,
          pinned: values.pinned,
          allow_comment: values.allow_comment,
          publish_time: values.publish_time || null,
        });
      }
      return updatePostContent(name, {
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
      void message.success("Saved");
      void qc.invalidateQueries({ queryKey: ["posts"] });
      if (isNew) {
        navigate({ to: "/posts/$name", params: { name: detail.name } });
      }
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Save failed"),
  });

  const publish = useMutation({
    mutationFn: async () => {
      if (published) return unpublishPost(name);
      return publishPost(name);
    },
    onSuccess: (detail) => {
      setPublished(detail.published);
      void message.success(detail.published ? "Published" : "Unpublished");
      void qc.invalidateQueries({ queryKey: ["posts"] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });

  return (
    <Card>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <Title level={3} style={{ margin: 0 }}>
            {isNew ? "New post" : post.data?.title || "Edit post"}
          </Title>
          <Space>
            <Button
              icon={<SettingOutlined />}
              onClick={() => setSettingsOpen(true)}
            >
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
            {!isNew && (
              <Button
                icon={<SendOutlined />}
                loading={publish.isPending}
                onClick={() => publish.mutate()}
              >
                {published ? "Unpublish" : "Publish"}
              </Button>
            )}
          </Space>
        </div>

        <Form form={form} layout="vertical">
          <Form.Item
            name="title"
            label="Title"
            rules={[{ required: true, message: "Title is required" }]}
          >
            <Input placeholder="An informative title" />
          </Form.Item>
          <MarkdownEditor initialMarkdown={markdown} onChange={setMarkdown} />
        </Form>
      </Space>
      <Drawer
        title="Post settings"
        open={settingsOpen}
        width={420}
        onClose={() => setSettingsOpen(false)}
        extra={
          <Button type="primary" loading={save.isPending} onClick={() => save.mutate()}>
            Save
          </Button>
        }
      >
        <Form form={form} layout="vertical">
          <Form.Item name="slug" label="Slug" tooltip="Defaults to a slug derived from the title">
            <Input placeholder="my-post" />
          </Form.Item>
          <Form.Item name="excerpt" label="Excerpt">
            <Input.TextArea rows={3} placeholder="A short summary shown on the homepage" />
          </Form.Item>
          <Form.Item name="visible" label="Visibility" initialValue="PUBLIC">
            <Select
              options={[
                { value: "PUBLIC", label: "Public" },
                { value: "INTERNAL", label: "Internal" },
                { value: "PRIVATE", label: "Private" },
              ]}
            />
          </Form.Item>
          <Form.Item name="cover" label="Cover">
            <Input placeholder="/upload/path/to-cover.jpg" />
          </Form.Item>
          <Form.Item name="template" label="Template">
            <Input placeholder="default" />
          </Form.Item>
          <Form.Item name="priority" label="Priority" initialValue={0}>
            <InputNumber style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item name="publish_time" label="Publish time">
            <Input placeholder="2026-05-19T00:00:00Z" />
          </Form.Item>
          <Form.Item name="pinned" label="Pinned" valuePropName="checked" initialValue={false}>
            <Switch />
          </Form.Item>
          <Form.Item name="allow_comment" label="Allow comments" valuePropName="checked" initialValue>
            <Switch />
          </Form.Item>
          {!isNew && (
            <Form.Item label="Published">
              <Switch
                checked={published}
                onChange={() => publish.mutate()}
                loading={publish.isPending}
              />
            </Form.Item>
          )}
        </Form>
      </Drawer>
    </Card>
  );
}
