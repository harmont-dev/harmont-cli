defmodule MyAppTest do
  use ExUnit.Case

  test "application starts" do
    assert {:ok, _pid} = Application.ensure_all_started(:my_app)
  end
end
