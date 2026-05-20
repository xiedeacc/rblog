import { useEffect, useRef, useState } from "react";
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
  Alert,
  Spin,
  Modal,
} from "antd";
import { ArrowLeftOutlined, SaveOutlined, SendOutlined, SettingOutlined } from "@ant-design/icons";
import { useNavigate, useParams, useSearch } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";
import {
  fetchPost,
  createPost,
  updatePostContent,
  publishPost,
  unpublishPost,
} from "@/api/client";
import { MarkdownEditor, MarkdownPreview } from "@/components/MarkdownEditor";

const { Text, Title } = Typography;

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

function compactPostsSearch(search: PostsSearch): PostsSearch {
  return {
    page: search.page === 1 ? undefined : search.page,
    size: search.size === 20 ? undefined : search.size,
    q: search.q || undefined,
    status: search.status,
    visible: search.visible,
    sort: search.sort,
    source: search.source,
    returnTo: search.returnTo,
  };
}

function generateUuid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}`;
}

function htmlToMarkdown(html: string): string {
  const turndown = new TurndownService({
    headingStyle: "atx",
    bulletListMarker: "-",
    codeBlockStyle: "fenced",
    emDelimiter: "_",
    strongDelimiter: "**",
  });

  turndown.use(gfm);
  turndown.keep(["iframe", "video", "audio"]);
  turndown.addRule("details", {
    filter: ["details"],
    replacement: (_content, node) => {
      const element = node as HTMLElement;
      const summary = element.querySelector("summary")?.textContent?.trim() || "Detail";
      const clone = element.cloneNode(true) as HTMLElement;
      clone.querySelector("summary")?.remove();
      const body = turndown.turndown(clone.innerHTML).trim();
      return `\n\n<details>\n<summary>${summary}</summary>\n\n${body}\n\n</details>\n\n`;
    },
  });
  turndown.addRule("preCode", {
    filter: (node) =>
      node.nodeName === "PRE" &&
      node.firstChild?.nodeName === "CODE",
    replacement: (_content, node) => {
      const code = node.firstChild as HTMLElement;
      const language = [...code.classList]
        .find((name) => name.startsWith("language-"))
        ?.replace("language-", "") ?? "";
      return `\n\n\`\`\`${language}\n${code.textContent ?? ""}\n\`\`\`\n\n`;
    },
  });

  return turndown
    .turndown(html)
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

export function PostEditPage() {
  const params = useParams({ strict: false }) as { name?: string };
  const isNew = !params.name;
  const name = params.name ?? "";
  const listSearch = compactPostsSearch(useSearch({ strict: false }) as PostsSearch);
  const navigate = useNavigate();
  const { message } = App.useApp();
  const qc = useQueryClient();

  const [form] = Form.useForm<FormValues>();
  const draftUuid = useRef(generateUuid());
  const [markdown, setMarkdown] = useState("");
  const [published, setPublished] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [convertOpen, setConvertOpen] = useState(false);
  const [convertedMarkdown, setConvertedMarkdown] = useState("");
  const [createdDraftName, setCreatedDraftName] = useState<string | null>(null);
  const [autoSaveDirty, setAutoSaveDirty] = useState(false);
  const [autoSaveStatus, setAutoSaveStatus] = useState("Draft autosave ready");

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
      setAutoSaveDirty(false);
      setAutoSaveStatus(post.data.published ? "Autosave disabled for published posts" : "Draft autosave ready");
    }
  }, [post.data, form]);

  const save = useMutation({
    mutationFn: async () => {
      const values = await form.validateFields();
      if (isNew) {
        const generatedName = values.name || createdDraftName || draftUuid.current;
        const slug = values.slug || draftUuid.current;
        if (createdDraftName) {
          return updatePostContent(generatedName, {
            markdown,
            title: values.title,
            slug,
            excerpt: values.excerpt,
            visible: values.visible,
            cover: values.cover,
            template: values.template,
            priority: values.priority,
            pinned: values.pinned,
            allow_comment: values.allow_comment,
            publish_time: values.publish_time || null,
          });
        }
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
      setAutoSaveDirty(false);
      setAutoSaveStatus("Saved");
      void message.success("Saved");
      void qc.invalidateQueries({ queryKey: ["posts"] });
      if (isNew) {
        navigate({ to: "/posts/$name", params: { name: detail.name }, search: listSearch });
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

  useEffect(() => {
    if (!autoSaveDirty || published || save.isPending || publish.isPending) return;

    const timer = window.setTimeout(() => {
      const run = async () => {
        const values = form.getFieldsValue();
        const title = values.title?.trim() || "Untitled draft";
        const hasDraftContent = Boolean(values.title?.trim() || markdown.trim());
        if (isNew && !hasDraftContent) return;

        const draftName = createdDraftName || (isNew ? draftUuid.current : name);
        const slug = values.slug?.trim() || (isNew ? draftUuid.current : post.data?.slug || draftName);
        setAutoSaveStatus("Autosaving draft...");

        try {
          const body = {
            markdown,
            title,
            slug,
            excerpt: values.excerpt,
            visible: values.visible,
            cover: values.cover,
            template: values.template,
            priority: values.priority,
            pinned: values.pinned,
            allow_comment: values.allow_comment,
            publish_time: values.publish_time || null,
          };
          const detail =
            isNew && !createdDraftName
              ? await createPost({ name: draftName, ...body })
              : await updatePostContent(draftName, body);

          setCreatedDraftName(detail.name);
          setAutoSaveDirty(false);
          setAutoSaveStatus(`Autosaved at ${new Date().toLocaleTimeString()}`);
          void qc.invalidateQueries({ queryKey: ["posts"] });
          if (isNew && !createdDraftName) {
            void navigate({
              to: "/posts/$name",
              params: { name: detail.name },
              search: listSearch,
              replace: true,
            });
          }
        } catch (error) {
          setAutoSaveStatus(error instanceof Error ? `Autosave failed: ${error.message}` : "Autosave failed");
        }
      };
      void run();
    }, 3000);

    return () => window.clearTimeout(timer);
  }, [
    autoSaveDirty,
    createdDraftName,
    form,
    isNew,
    listSearch,
    markdown,
    name,
    navigate,
    post.data?.slug,
    published,
    publish.isPending,
    qc,
    save.isPending,
  ]);

  if (!isNew && post.isLoading) {
    return (
      <Card>
        <Spin />
      </Card>
    );
  }

  if (!isNew && post.isError) {
    return (
      <Card>
        <Alert
          type="error"
          showIcon
          message="Failed to load post"
          description={post.error instanceof Error ? post.error.message : "Unable to fetch this post."}
        />
      </Card>
    );
  }

  const rawType = (post.data?.raw_type ?? "markdown").toLowerCase();
  const isHtmlPost = rawType === "html";
  const hasMissingSnapshot =
    !isNew && post.data && !post.data.raw_markdown && !post.data.content_html;
  const backToPosts = () => {
    if (listSearch.returnTo?.startsWith("/")) {
      window.location.assign(listSearch.returnTo);
      return;
    }
    const target = listSearch.source === "deleted" ? "/posts/deleted" : "/posts";
    void navigate({ to: target, search: { ...listSearch, source: undefined, returnTo: undefined } });
  };
  const openConvertPreview = () => {
    setConvertedMarkdown(htmlToMarkdown(markdown || post.data?.content_html || ""));
    setConvertOpen(true);
  };
  const confirmConvert = () => {
    setMarkdown(convertedMarkdown);
    setAutoSaveDirty(true);
    setConvertOpen(false);
    void message.success("Converted to Markdown. Click Save to persist the change.");
  };

  const handleMarkdownChange = (next: string) => {
    setMarkdown(next);
    setAutoSaveDirty(true);
  };

  return (
    <Card>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <Title level={3} style={{ margin: 0 }}>
            {isNew ? "New post" : post.data?.title || "Edit post"}
          </Title>
          <Space>
            <Button icon={<ArrowLeftOutlined />} onClick={backToPosts}>
              Back
            </Button>
            <Button
              icon={<SettingOutlined />}
              onClick={() => setSettingsOpen(true)}
            >
              Setting
            </Button>
            {isHtmlPost ? (
              <Button onClick={openConvertPreview}>
                Convert to Markdown
              </Button>
            ) : null}
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

        {!published ? (
          <Text type={autoSaveStatus.startsWith("Autosave failed") ? "danger" : "secondary"}>
            {autoSaveStatus}
          </Text>
        ) : null}

        {hasMissingSnapshot ? (
          <Alert
            type="warning"
            showIcon
            message="This imported post has no content snapshot"
            description="The original data contains the post metadata, but no base/head/release snapshot, so there is no body content to show."
          />
        ) : null}

        {rawType !== "markdown" ? (
          <Alert
            type="info"
            showIcon
            message={`This post was imported as ${rawType.toUpperCase()}`}
            description="The editor shows the original source format on the left. For HTML posts, that means HTML source rather than Markdown."
            action={isHtmlPost ? <Button size="small" onClick={openConvertPreview}>Convert</Button> : undefined}
          />
        ) : null}

        <Form form={form} layout="vertical" onValuesChange={() => setAutoSaveDirty(true)}>
          <Form.Item
            name="title"
            label="Title"
            rules={[{ required: true, message: "Title is required" }]}
          >
            <Input placeholder="An informative title" />
          </Form.Item>
          <MarkdownEditor initialMarkdown={markdown} onChange={handleMarkdownChange} />
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
          <Form.Item name="slug" label="Slug" tooltip="Defaults to a UUID and is used as the public post permalink key">
            <Input placeholder={draftUuid.current} />
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
      <Modal
        title="Preview Markdown Conversion"
        open={convertOpen}
        width="90vw"
        onCancel={() => setConvertOpen(false)}
        footer={[
          <Button key="cancel" onClick={() => setConvertOpen(false)}>
            Cancel
          </Button>,
          <Button key="confirm" type="primary" onClick={confirmConvert}>
            Confirm Conversion
          </Button>,
        ]}
      >
        <div className="html-convert-preview">
          <div className="html-convert-preview__pane">
            <div className="html-convert-preview__title">Converted Markdown</div>
            <textarea
              className="html-convert-preview__source"
              value={convertedMarkdown}
              onChange={(event) => setConvertedMarkdown(event.target.value)}
            />
          </div>
          <div className="html-convert-preview__pane">
            <div className="html-convert-preview__title">Rendered Preview</div>
            <MarkdownPreview
              markdown={convertedMarkdown}
              className="markdown-preview html-convert-preview__rendered"
            />
          </div>
        </div>
      </Modal>
    </Card>
  );
}
