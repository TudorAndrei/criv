defmodule Acceptance.BeforeError do
  def safe_before(), do: :ok
  def broken(, do: :error)
  def safe_after(), do: :ok
end

defmodule Acceptance.AfterError do
  def safe(), do: :ok
end
