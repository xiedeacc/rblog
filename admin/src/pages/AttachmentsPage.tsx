import { Card, Image, Popconfirm, Space, Typography, Upload, App } from "antd";
import { InboxOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listAttachments, uploadAttachment, removeAttachment } from "@/api/client";

const { Title, Paragraph, Text } = Typography;
const { Dragger } = Upload;

function isImageKey(key: string): boolean {
  return /\.(png|jpe?g|gif|webp|svg)$/i.test(key);
}

function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function AttachmentsPage() {
  const { message } = App.useApp();
  const qc = useQueryClient();
  const query = useQuery({
    queryKey: ["attachments"],
    queryFn: () => listAttachments(),
  });

  const onDelete = useMutation({
    mutationFn: removeAttachment,
    onSuccess: () => {
      void message.success("Attachment removed");
      void qc.invalidateQueries({ queryKey: ["attachments"] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });

  return (
    <Card>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Title level={3} style={{ margin: 0 }}>
          Attachments
        </Title>
        <Dragger
          multiple
          showUploadList={false}
          customRequest={async (opt) => {
            try {
              await uploadAttachment(opt.file as File);
              opt.onSuccess?.({});
              void qc.invalidateQueries({ queryKey: ["attachments"] });
              void message.success("Uploaded");
            } catch (e) {
              opt.onError?.(e instanceof Error ? e : new Error("upload failed"));
              void message.error(e instanceof Error ? e.message : "Upload failed");
            }
          }}
        >
          <p className="ant-upload-drag-icon">
            <InboxOutlined />
          </p>
          <Paragraph>Click or drag files to upload.</Paragraph>
          <Paragraph type="secondary">
            Files are stored via the configured backend (local FS or S3).
          </Paragraph>
        </Dragger>

        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(180px, 1fr))",
            gap: 16,
          }}
        >
          {(query.data ?? []).map((a) => (
            <Card
              key={a.key}
              size="small"
              cover={
                isImageKey(a.key) ? (
                  <Image
                    src={a.url}
                    alt={a.key}
                    style={{ aspectRatio: "1 / 1", objectFit: "cover" }}
                    placeholder
                  />
                ) : (
                  <div style={{ padding: 24, textAlign: "center" }}>
                    <Text type="secondary">{a.key.split(".").pop()?.toUpperCase() ?? "FILE"}</Text>
                  </div>
                )
              }
              actions={[
                <a key="open" href={a.url} target="_blank" rel="noreferrer">
                  Open
                </a>,
                <Popconfirm
                  key="del"
                  title="Delete attachment?"
                  onConfirm={() => onDelete.mutate(a.key)}
                >
                  <a style={{ color: "#cf1322" }}>Delete</a>
                </Popconfirm>,
              ]}
            >
              <Card.Meta
                title={<span title={a.key}>{a.key.split("/").pop()}</span>}
                description={humanSize(a.size)}
              />
            </Card>
          ))}
        </div>
      </Space>
    </Card>
  );
}
