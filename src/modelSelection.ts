export type SelectableModel = {
  id: string;
  displayName: string;
  description: string;
  isDefault: boolean;
  supportedReasoningEfforts: string[];
};

export function resolveManualModelFallback(
  fallbackModel: string,
  validatesModelList: boolean,
  reason?: string,
): { model: SelectableModel; notice: string } | null {
  const id = fallbackModel.trim();
  if (validatesModelList || !id) return null;
  return {
    model: {
      id,
      displayName: id,
      description: "手动配置模型",
      isDefault: true,
      supportedReasoningEfforts: [],
    },
    notice: reason
      ? `未校验服务端模型列表：${reason}`
      : "模型列表不可用，将使用手动填写的模型",
  };
}
