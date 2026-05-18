import { useState } from "react";
import { Button, Card, Form, Input, Modal, Space, Table, Tag, Typography, App, Popconfirm } from "antd";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  listUsers,
  createUser,
  disableUser,
  enableUser,
  setUserPassword,
  removeUser,
  type UserItem,
} from "@/api/client";

const { Title } = Typography;

export function UsersPage() {
  const { message } = App.useApp();
  const qc = useQueryClient();
  const [openCreate, setOpenCreate] = useState(false);
  const [pwdFor, setPwdFor] = useState<UserItem | null>(null);
  const [form] = Form.useForm();
  const [pwdForm] = Form.useForm();
  const query = useQuery({ queryKey: ["users"], queryFn: listUsers });

  const onCreate = useMutation({
    mutationFn: createUser,
    onSuccess: () => {
      void message.success("User created");
      setOpenCreate(false);
      form.resetFields();
      void qc.invalidateQueries({ queryKey: ["users"] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });
  const onToggle = useMutation({
    mutationFn: async (u: UserItem) => (u.disabled ? enableUser(u.name) : disableUser(u.name)),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["users"] }),
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });
  const onDelete = useMutation({
    mutationFn: removeUser,
    onSuccess: () => {
      void message.success("User deleted");
      void qc.invalidateQueries({ queryKey: ["users"] });
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });
  const onPwd = useMutation({
    mutationFn: ({ name, password }: { name: string; password: string }) =>
      setUserPassword(name, password),
    onSuccess: () => {
      void message.success("Password updated");
      setPwdFor(null);
      pwdForm.resetFields();
    },
    onError: (e) => void message.error(e instanceof Error ? e.message : "Failed"),
  });

  return (
    <Card>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <div style={{ display: "flex", justifyContent: "space-between" }}>
          <Title level={3} style={{ margin: 0 }}>
            Users
          </Title>
          <Button type="primary" onClick={() => setOpenCreate(true)}>
            New user
          </Button>
        </div>
        <Table<UserItem>
          rowKey="name"
          loading={query.isLoading}
          dataSource={query.data ?? []}
          pagination={false}
          columns={[
            { title: "Username", dataIndex: "name" },
            { title: "Display", dataIndex: "display_name" },
            { title: "Email", dataIndex: "email" },
            { title: "Registered", dataIndex: "registered_at" },
            {
              title: "Status",
              key: "status",
              render: (_v: unknown, row: UserItem) =>
                row.disabled ? <Tag>disabled</Tag> : <Tag color="green">active</Tag>,
            },
            {
              title: "Actions",
              key: "actions",
              render: (_v: unknown, row: UserItem) => (
                <Space>
                  <Button size="small" onClick={() => setPwdFor(row)}>
                    Set password
                  </Button>
                  <Button size="small" onClick={() => onToggle.mutate(row)}>
                    {row.disabled ? "Enable" : "Disable"}
                  </Button>
                  <Popconfirm title="Delete user?" onConfirm={() => onDelete.mutate(row.name)}>
                    <Button size="small" danger>
                      Delete
                    </Button>
                  </Popconfirm>
                </Space>
              ),
            },
          ]}
        />
      </Space>

      <Modal
        title="New user"
        open={openCreate}
        onCancel={() => setOpenCreate(false)}
        onOk={() => form.submit()}
        confirmLoading={onCreate.isPending}
      >
        <Form form={form} layout="vertical" onFinish={(v) => onCreate.mutate(v)}>
          <Form.Item name="name" label="Username" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="display_name" label="Display name" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="email" label="Email" rules={[{ required: true, type: "email" }]}>
            <Input />
          </Form.Item>
          <Form.Item name="password" label="Password" rules={[{ required: true, min: 8 }]}>
            <Input.Password />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={`Set password for ${pwdFor?.display_name ?? ""}`}
        open={!!pwdFor}
        onCancel={() => setPwdFor(null)}
        onOk={() => pwdForm.submit()}
        confirmLoading={onPwd.isPending}
      >
        <Form
          form={pwdForm}
          layout="vertical"
          onFinish={(v: { password: string }) => {
            if (pwdFor) onPwd.mutate({ name: pwdFor.name, password: v.password });
          }}
        >
          <Form.Item name="password" label="New password" rules={[{ required: true, min: 8 }]}>
            <Input.Password />
          </Form.Item>
        </Form>
      </Modal>
    </Card>
  );
}
