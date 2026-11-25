// .vue imports for plain tsc-based tools (vue-tsc reads the components themselves)
declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<object, object, any>;
  export default component;
}
