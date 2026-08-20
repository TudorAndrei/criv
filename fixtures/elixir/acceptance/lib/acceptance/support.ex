defmodule Acceptance.Repo do
  def fetch(value), do: {:ok, value}
  def fetch(value, opts), do: {:ok, {value, opts}}
  defp normalize(value), do: value
end

defmodule Acceptance.Helpers do
  def tag(value), do: {:tagged, value}
end

defmodule Acceptance.Macros do
  defmacro build(value), do: value
end

defmodule Acceptance.Dsl do
  defmacro __using__(_opts), do: :ok
end
