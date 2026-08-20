defimpl Acceptance.Renderable, for: Acceptance.User do
  def render(value), do: inspect(value)
end
